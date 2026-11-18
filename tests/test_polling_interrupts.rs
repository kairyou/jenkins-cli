use jenkins::jenkins::{client::JenkinsClient, Event};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};

struct Request {
    path: String,
    response: oneshot::Sender<String>,
}

struct Server {
    url: String,
    requests: mpsc::Receiver<Request>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (requests, receiver) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let requests = requests.clone();
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 1024];
                    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        let count = socket.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    let path = String::from_utf8(bytes)
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .to_string();
                    let (response, body) = oneshot::channel();
                    requests.send(Request { path, response }).await.unwrap();
                    let body = body.await.unwrap();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nX-Text-Size: 10\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        Self {
            url,
            requests: receiver,
            task,
        }
    }

    async fn next_request(&mut self) -> Request {
        timeout(Duration::from_secs(5), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn build_pause_waits_for_in_flight_output_and_resumes_polling() {
    let mut server = Server::new().await;
    let client = JenkinsClient::new(&server.url, None, None, None, None);
    let build_url = format!("{}/job/example/1", server.url);
    let (sender, mut receiver) = mpsc::channel(8);
    let polling = tokio::spawn(async move { client.poll_build_status(&build_url, &mut receiver).await });

    let status = server.next_request().await;
    assert!(status.path.ends_with("/api/json"));
    let (acknowledged, mut paused) = oneshot::channel();
    sender.send(Event::StopSpinner(acknowledged)).await.unwrap();
    assert!(timeout(Duration::from_millis(100), &mut paused).await.is_err());
    status.response.send(r#"{"building":true}"#.into()).unwrap();
    let console = server.next_request().await;
    assert!(console.path.contains("/logText/progressiveHtml"));
    assert!(console.path.ends_with("start=0"));
    assert!(timeout(Duration::from_millis(100), &mut paused).await.is_err());
    console.response.send("first log line\n".into()).unwrap();
    timeout(Duration::from_secs(3), paused).await.unwrap().unwrap();

    assert!(timeout(Duration::from_millis(800), server.requests.recv())
        .await
        .is_err());
    sender.send(Event::ResumeSpinner).await.unwrap();
    server
        .next_request()
        .await
        .response
        .send(r#"{"building":false,"result":"SUCCESS"}"#.into())
        .unwrap();
    let remaining_console = server.next_request().await;
    assert!(remaining_console.path.ends_with("start=10"));
    remaining_console.response.send("final log line\n".into()).unwrap();
    timeout(Duration::from_secs(3), polling)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn queue_pause_stays_paused_until_explicit_resume() {
    let mut server = Server::new().await;
    let mut client = JenkinsClient::new(&server.url, None, None, None, None);
    client.job_url = Some(format!("{}/job/example", server.url));
    let queue_url = format!("{}/queue/item/1", server.url);
    let (sender, mut receiver) = mpsc::channel(8);
    let (acknowledged, paused) = oneshot::channel();
    sender.send(Event::StopSpinner(acknowledged)).await.unwrap();
    let polling = tokio::spawn(async move { client.poll_queue_item(&queue_url, &mut receiver).await });
    timeout(Duration::from_secs(2), paused).await.unwrap().unwrap();
    assert!(timeout(Duration::from_millis(2100), server.requests.recv())
        .await
        .is_err());
    sender.send(Event::ResumeSpinner).await.unwrap();
    server
        .next_request()
        .await
        .response
        .send(r#"{"executable":{"number":1}}"#.into())
        .unwrap();
    let url = timeout(Duration::from_secs(3), polling)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(url, format!("{}/job/example/1", server.url));
}

#[tokio::test]
async fn retry_pause_waits_for_resume_and_allows_cancellation() {
    let mut server = Server::new().await;
    let client = JenkinsClient::new(&server.url, None, None, None, None);
    let build_url = format!("{}/job/example/1", server.url);
    let (sender, mut receiver) = mpsc::channel(8);
    let polling = tokio::spawn(async move { client.poll_build_status(&build_url, &mut receiver).await });
    let status = server.next_request().await;
    let (acknowledged, paused) = oneshot::channel();
    sender.send(Event::StopSpinner(acknowledged)).await.unwrap();
    status.response.send("invalid JSON".into()).unwrap();
    timeout(Duration::from_secs(3), paused).await.unwrap().unwrap();
    assert!(timeout(Duration::from_millis(1100), server.requests.recv())
        .await
        .is_err());
    sender.send(Event::CancelPolling).await.unwrap();
    let error = timeout(Duration::from_secs(3), polling)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("cancelled!"));
}

#[tokio::test]
async fn cancellation_checks_and_stops_only_the_polled_build() {
    let mut server = Server::new().await;
    let mut client = JenkinsClient::new(&server.url, None, None, None, None);
    client.job_url = Some(format!("{}/job/example", server.url));
    let client = std::sync::Arc::new(client);
    let build_url = format!("{}/job/example/1", server.url);
    let (sender, mut receiver) = mpsc::channel(8);
    let (acknowledged, paused) = oneshot::channel();
    sender.send(Event::StopSpinner(acknowledged)).await.unwrap();
    let polling_client = client.clone();
    let polling = tokio::spawn(async move { polling_client.poll_build_status(&build_url, &mut receiver).await });
    timeout(Duration::from_secs(2), paused).await.unwrap().unwrap();

    // Even when lastBuild could now refer to a newer run, query the original run.
    let status_client = client.clone();
    let status = tokio::spawn(async move { status_client.is_building().await });
    let request = server.next_request().await;
    assert_eq!(request.path, "/job/example/1/api/json");
    request
        .response
        .send(r#"{"building":false,"number":1,"result":"SUCCESS"}"#.into())
        .unwrap();
    let status = status.await.unwrap().unwrap();
    assert!(!status.building);
    assert_eq!(status.id, Some(1));

    // Stop and verification requests must also retain the original run's URL.
    let stop_client = client.clone();
    let stop = tokio::spawn(async move { stop_client.cancel_build(None).await });
    let request = server.next_request().await;
    assert_eq!(request.path, "/job/example/1/stop");
    request.response.send(String::new()).unwrap();
    stop.await.unwrap().unwrap();
    sender.send(Event::CancelPolling).await.unwrap();
    assert!(polling.await.unwrap().is_err());
}

// https://pages.edgeone.ai/document/edge-functions
export default async function onRequest() {
  return new Response("<h1>functions</h1>", {
    status: 200,
    headers: { "content-type": "text/html;charset=UTF-8" }
  });
}

// use std::borrow::Cow;
// use std::io::BufReader;
use jenkins::jenkins::{
    parse_jenkins_job_parameter,
    // JenkinsJobParameter
};

#[test]
fn main() {
    let xml_data = r#"
        <flow-definition plugin="workflow-job@1308.v58d48a_763b_31">
            <properties>
                <hudson.model.ParametersDefinitionProperty>
                    <parameterDefinitions>
                        <hudson.model.StringParameterDefinition>
                            <name>Git_Branch</name>
                            <defaultValue>master</defaultValue>
                            <trim>true</trim>
                        </hudson.model.StringParameterDefinition>
                        <hudson.model.ChoiceParameterDefinition>
                            <name>APP_ENV</name>
                            <choices class="java.util.Arrays$ArrayList">
                                <a class="string-array">
                                    <string>sit</string>
                                    <string>uat</string>
                                </a>
                            </choices>
                        </hudson.model.ChoiceParameterDefinition>
                        <hudson.model.BooleanParameterDefinition>
                            <name>Boolean test</name>
                            <description>Boolean!</description>
                            <defaultValue>true</defaultValue>
                        </hudson.model.BooleanParameterDefinition>
                        <hudson.model.FileParameterDefinition>
                            <name>File test</name>
                            <description>File!</description>
                        </hudson.model.FileParameterDefinition>
                        <hudson.model.TextParameterDefinition>
                            <name>Multi-line test</name>
                            <description>Multi-line</description>
                            <defaultValue>Multi-line Multi-line</defaultValue>
                            <trim>false</trim>
                        </hudson.model.TextParameterDefinition>
                        <hudson.model.PasswordParameterDefinition>
                            <name>Password test</name>
                            <description>Password</description>
                            <defaultValue>{AQAAABAAAAAQcrJMptYjOKgrP/MgQtgtUApDcvwu65D01Zerc7evgF4=}</defaultValue>
                        </hudson.model.PasswordParameterDefinition>
                        <com.cloudbees.plugins.credentials.CredentialsParameterDefinition plugin="credentials@1254.vb_96f366e7b_a_d">
                            <name>Credentials test</name>
                            <description>Credentials!</description>
                            <defaultValue>6a1653e8-77a5-4fc2-a5cb-949663237aec</defaultValue>
                            <credentialType>com.cloudbees.plugins.credentials.impl.UsernamePasswordCredentialsImpl</credentialType>
                            <required>true</required>
                        </com.cloudbees.plugins.credentials.CredentialsParameterDefinition>
                        <hudson.model.RunParameterDefinition>
                            <name>Run test</name>
                            <description>Run!</description>
                            <projectName>project: example-job</projectName>
                            <filter>ALL</filter>
                        </hudson.model.RunParameterDefinition>
                    </parameterDefinitions>
                </hudson.model.ParametersDefinitionProperty>
            </properties>
        </flow-definition>
    "#;

    // quick_xml::de::from_str::<JenkinsJobConfig>(xml_data).unwrap(); 解析array数据 可能有问题(choices 部分)
    let parameters = parse_jenkins_job_parameter(xml_data);
    println!("{:#?}", parameters);
}

use jenkins::{
    constants::DEFAULT_PARAM_VALUE,
    jenkins::{parse_job_parameters_from_json, parse_job_parameters_from_xml},
};
use serde_json::json;

#[test]
fn parse_parameters_from_xml() {
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
                                    <string/>
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

    let parameters = parse_job_parameters_from_xml(xml_data);

    assert_eq!(parameters.len(), 5);
    assert_eq!(parameters[0].name, "Git_Branch");
    assert_eq!(parameters[0].default_value.as_deref(), Some("master"));
    assert_eq!(parameters[0].trim, Some(true));

    let choice_values = parameters[1].choices.as_ref().expect("choice values exist");
    assert_eq!(choice_values.len(), 3);
    assert_eq!(choice_values[0], "");

    let password_param = parameters
        .iter()
        .find(|param| param.name == "Password test")
        .expect("password param exists");
    assert_eq!(password_param.default_value.as_deref(), Some(DEFAULT_PARAM_VALUE));
}

#[test]
fn parse_parameters_from_json() {
    let json_data = json!({
        "property": [
            {
                "_class": "hudson.model.ParametersDefinitionProperty",
                "parameterDefinitions": [
                    {
                        "_class": "hudson.model.StringParameterDefinition",
                        "defaultParameterValue": {
                            "_class": "hudson.model.StringParameterValue",
                            "value": "main"
                        },
                        "description": "git branch",
                        "name": "GIT_BRANCH",
                        "type": "StringParameterDefinition"
                    },
                    {
                        "_class": "hudson.model.PasswordParameterDefinition",
                        "defaultParameterValue": {
                            "_class": "hudson.model.PasswordParameterValue"
                        },
                        "description": "password parameter test",
                        "name": "PASSWORD",
                        "type": "PasswordParameterDefinition"
                    },
                    {
                        "_class": "hudson.model.FileParameterDefinition",
                        "name": "FILE_UPLOAD",
                        "type": "FileParameterDefinition"
                    },
                    {
                        "_class": "hudson.model.ChoiceParameterDefinition",
                        "defaultParameterValue": {
                            "_class": "hudson.model.StringParameterValue",
                            "value": "sit"
                        },
                        "description": "app",
                        "name": "APP_ENV",
                        "type": "ChoiceParameterDefinition",
                        "choices": [
                            "sit",
                            "uat",
                            "prod"
                        ]
                    },
                    {
                        "_class": "hudson.model.BooleanParameterDefinition",
                        "defaultParameterValue": {
                            "_class": "hudson.model.BooleanParameterValue",
                            "value": true
                        },
                        "description": "is debug",
                        "name": "IS_DEBUG",
                        "type": "BooleanParameterDefinition"
                    },
                    {
                        "_class": "hudson.model.TextParameterDefinition",
                        "defaultParameterValue": {
                            "_class": "hudson.model.TextParameterValue",
                            "value": "aa\nbb"
                        },
                        "description": null,
                        "name": "Multi-line",
                        "type": "TextParameterDefinition"
                    },
                    {
                        "_class": "com.cloudbees.plugins.credentials.CredentialsParameterDefinition",
                        "defaultParameterValue": {
                            "_class": "com.cloudbees.plugins.credentials.CredentialsParameterValue"
                        },
                        "description": null,
                        "name": "Credentials",
                        "type": "CredentialsParameterDefinition"
                    },
                    {
                        "_class": "hudson.model.RunParameterDefinition",
                        "name": "RUN_BUILD",
                        "type": "RunParameterDefinition",
                        "projectName": "example-job"
                    }
                ]
            }
        ]
    });

    let parameters = parse_job_parameters_from_json(&json_data);
    // FILE_UPLOAD, Credentials and RUN_BUILD should be filtered out.
    assert_eq!(parameters.len(), 5);

    let string_param = parameters
        .iter()
        .find(|param| param.name == "GIT_BRANCH")
        .expect("string param exists");
    assert_eq!(string_param.default_value.as_deref(), Some("main"));

    let password_param = parameters
        .iter()
        .find(|param| param.name == "PASSWORD")
        .expect("password param exists");
    assert_eq!(password_param.default_value.as_deref(), Some(DEFAULT_PARAM_VALUE));

    let text_param = parameters
        .iter()
        .find(|param| param.name == "Multi-line")
        .expect("text param exists");
    assert_eq!(text_param.default_value.as_deref(), Some("aa\nbb"));
    assert_eq!(text_param.trim, None);

    let choice_param = parameters
        .iter()
        .find(|param| param.name == "APP_ENV")
        .expect("choice param exists");
    assert_eq!(choice_param.choices.as_ref().map(|choices| choices.len()), Some(3));

    let boolean_param = parameters
        .iter()
        .find(|param| param.name == "IS_DEBUG")
        .expect("boolean param exists");
    assert_eq!(boolean_param.default_value.as_deref(), Some("true"));

    assert!(parameters.iter().all(|param| param.name != "Credentials"));
    assert!(parameters.iter().all(|param| param.name != "RUN_BUILD"));
    assert!(parameters.iter().all(|param| param.name != "FILE_UPLOAD"));
}

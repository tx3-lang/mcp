use std::sync::Arc;
use std::collections::HashMap;
use serde_json::Map;
use rmcp::{Error as McpError, ServerHandler, RoleServer, tool};
use rmcp::service::{RequestContext, Peer};
use rmcp::model::*;
use cynic;
use cynic::QueryBuilder;
use cynic::http::SurfExt;
use tx3_sdk::trp::{Client as TrpClient, ClientOptions, ProtoTxRequest, TirInfo};

#[cynic::schema("tx3")]
mod schema {}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct ProtocolsQuery {
    pub dapps: DappConnection,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct DappConnection {
    pub nodes: Vec<Dapp>,
}

#[derive(cynic::QueryFragment, Debug)]
pub struct Dapp {
    pub scope: String,
    pub name: String,
    pub protocol: Option<String>,
}


#[derive(Clone)]
pub struct Protocol {
    name: String,
    content: String,
}

#[derive(Clone)]
pub struct ProtocolTool {
    registry_url: String,
    trp_url: String,
    trp_key: String,
}

#[tool(tool_box)]
impl ProtocolTool {
    #[allow(dead_code)]
    pub fn new(registry_url: &str, trp_url: &str, trp_key: &str) -> Self {
        Self {
            registry_url: registry_url.to_string(),
            trp_url: trp_url.to_string(),
            trp_key: trp_key.to_string(),
        }
    }

    async fn run_protocols_query(&self) -> Vec<Protocol> {
        let query = ProtocolsQuery::build({});
        let response = surf::post(self.registry_url.clone()).run_graphql(query).await.unwrap().data;
        match response {
            Some(data) => data.dapps.nodes.into_iter()
                .filter(|dapp| dapp.protocol.is_some())
                .map(|dapp| {
                    Protocol {
                        name: format!("{}_{}", dapp.scope, dapp.name),
                        content: dapp.protocol.unwrap(),
                    }
                })
                .collect(),
            None => Vec::new(),
        }
    }
}

impl ServerHandler for ProtocolTool {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("This server provides a protocol tool that can be use to comunicate with tx3 files for listing and resolving the transactions inside them.".to_string()),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {

        let protocols = self.run_protocols_query().await;

        let mut property = Map::new();
        property.insert("type".to_string(), serde_json::Value::String("string".to_string()));

        let mut tools = Vec::new();
        for protocol in protocols.iter() {
            let tx3_protocol = tx3_lang::Protocol::from_string(protocol.content.to_string()).load().unwrap();
            for tx in tx3_protocol.txs() {
                let prototx = tx3_protocol.new_tx(tx.name.as_str()).unwrap();
                let mut properties = Map::new();
                let mut required = Vec::new();
                for param in prototx.find_params() {
                    properties.insert(param.0.clone(), serde_json::Value::Object(property.clone()));
                    required.push(serde_json::Value::String(param.0.clone()));  
                }

                let mut input_schema = Map::new();
                input_schema.insert("type".to_string(), serde_json::Value::String("object".to_string()));
                input_schema.insert("$schema".to_string(), serde_json::Value::String("http://json-schema.org/draft-07/schema#".to_string()));
                input_schema.insert("title".to_string(), serde_json::Value::String(format!("resolve_{}_{}_params", protocol.name.clone(), tx.name)));
                input_schema.insert("properties".to_string(), serde_json::Value::Object(properties));
                input_schema.insert("required".to_string(), serde_json::Value::Array(required));

                tools.push(Tool {
                    name: std::borrow::Cow::Owned(format!("resolve-{}-{}", protocol.name.clone(), tx.name)),
                    description: Some(std::borrow::Cow::Owned(format!("Resolves the transaction '{}' from the protocol '{}'", tx.name, protocol.name))),
                    annotations: Some(ToolAnnotations {
                        title: Some(format!("Resolve {} {}", protocol.name, tx.name)),
                        read_only_hint: Some(true),
                        destructive_hint: Some(false),
                        idempotent_hint: Some(false),
                        open_world_hint: Some(true),
                    }),
                    input_schema: Arc::new(input_schema),
                });

                tools.push(Tool {
                    name: std::borrow::Cow::Owned(format!("describe-{}-{}", protocol.name.clone(), tx.name)),
                    description: Some(std::borrow::Cow::Owned(format!("Describes the transaction '{}' from the protocol '{}' and shows the required parameters", tx.name, protocol.name))),
                    annotations: Some(ToolAnnotations {
                        title: Some(format!("Describe {} {}", protocol.name, tx.name)),
                        read_only_hint: Some(true),
                        destructive_hint: Some(false),
                        idempotent_hint: Some(false),
                        open_world_hint: Some(true),
                    }),
                    input_schema: Arc::new(Map::new()),
                });
            }
        }
        Ok(ListToolsResult { tools, next_cursor: None })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let name = request.name.split("-").collect::<Vec<&str>>();

        let operation_name = name.get(0)
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Operation not found"),
                    None,
                )
            })
            .unwrap().to_string();

        let protocol_name = name.get(1)
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Protocol name not found"),
                    None,
                )
            })
            .unwrap().to_string();

        let transaction_name = name.get(2)
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Transaction name not found"),
                    None,
                )
            })
            .unwrap().to_string();

        let protocols = self.run_protocols_query().await;
        let protocol = protocols.iter().find(|p| p.name == protocol_name).ok_or_else(|| {
            McpError::new(
                ErrorCode::RESOURCE_NOT_FOUND,
                format!("Protocol {} not found", protocol_name),
                None,
            )
        }).unwrap();

        let prototx = {
            let tx3_protocol = tx3_lang::Protocol::from_string(protocol.content.to_string()).load().unwrap();
            let prototx_result = tx3_protocol.new_tx(transaction_name.as_str());
            if prototx_result.is_err() {
                return Err(McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Transaction {} not found for protocol {}", transaction_name, protocol_name),
                    None,
                ));
            }
            prototx_result.unwrap()
        };

        let parameters_types = prototx.find_params();

        if operation_name == "describe" {
            let mut parameters = Map::new();
            for (param_name, param_type) in parameters_types.iter() {
                parameters.insert(param_name.clone(), serde_json::Value::String(format!("{:?}", param_type)));
            }
            let mut response = Map::new();
            response.insert("protocol".to_string(), serde_json::Value::String(protocol_name));
            response.insert("transaction".to_string(), serde_json::Value::String(transaction_name));
            response.insert("parameters".to_string(), serde_json::Value::Object(parameters));
            return Ok(CallToolResult::success(vec![Content::json(response)?]));
        }

        let parameters = request.arguments.is_some()
            .then(|| request.arguments.unwrap())
            .unwrap_or_default();

        let mut args: HashMap<String, tx3_lang::ArgValue> = HashMap::new();
        for (arg_name, value) in parameters.iter() {
            let string_value = value.as_str().ok_or_else(|| {
                McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Invalid value provided for parameter {}", arg_name),
                    None
                )
            }).unwrap();

            let arg_type = parameters_types.get(arg_name).ok_or_else(|| {
                McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Parameter {} not found for transaction {} in protocol {}", arg_name, transaction_name, protocol_name),
                    None
                )
            }).unwrap();

            let mut arg_value: Option<tx3_lang::ArgValue> = None;
            if *arg_type == tx3_lang::ir::Type::Int {
                arg_value = Some(tx3_lang::ArgValue::Int(string_value.parse::<i128>().unwrap()));
            }
            if *arg_type == tx3_lang::ir::Type::Bool {
                arg_value = Some(tx3_lang::ArgValue::Bool(string_value.parse::<bool>().unwrap()));
            }
            if *arg_type == tx3_lang::ir::Type::Bytes {
                arg_value = Some(tx3_lang::ArgValue::String(string_value.to_string()));
            }
            if *arg_type == tx3_lang::ir::Type::Address {
                arg_value = Some(tx3_lang::ArgValue::String(string_value.to_string()));
            }

            if arg_value.is_none() {
                return Err(McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Invalid value provided for parameter {}", arg_name),
                    None
                ));
            }

            args.insert(arg_name.clone(), arg_value.unwrap());
        }

        let client = TrpClient::new(ClientOptions {
            endpoint: self.trp_url.clone(),
            headers: Some(HashMap::from([("dmtr-api-key".to_string(), self.trp_key.clone())])),
            env_args: None,
        });

        let result = client.resolve(ProtoTxRequest {
            tir: TirInfo {
                bytecode: hex::encode(prototx.ir_bytes()),
                encoding: "hex".to_string(),
                version: tx3_lang::ir::IR_VERSION.to_string(),
            },
            args: serde_json::to_value(args).unwrap()
        }).await;

        if result.is_err() {
            return Err(McpError::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Error resolving transaction: {}", result.unwrap_err()),
                None
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(result.unwrap().tx)]))
    }

    fn ping(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), McpError>> + Send + '_ {
        std::future::ready(Ok(()))
    }
    
    fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        std::future::ready(Ok(self.get_info()))
    }

    fn complete(
        &self,
        _request: CompleteRequestParam, 
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CompleteResult, McpError>> + Send + '_ {
        std::future::ready(Err(McpError::method_not_found::<CompleteRequestMethod>()))
    }

    fn set_level(
        &self,
        _request: SetLevelRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), McpError>> + Send + '_ {
        std::future::ready(Err(McpError::method_not_found::<SetLevelRequestMethod>()))
    }

    fn get_prompt(
        &self,
        _request: GetPromptRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetPromptResult, McpError>> + Send + '_ {
        std::future::ready(Err(McpError::method_not_found::<GetPromptRequestMethod>()))
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListPromptsResult::default()))
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult::default()))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourceTemplatesResult::default()))
    }

    fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        std::future::ready(Err(
            McpError::method_not_found::<ReadResourceRequestMethod>(),
        ))
    }

    fn subscribe(
        &self,
        _request: SubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), McpError>> + Send + '_ {
        std::future::ready(Err(McpError::method_not_found::<SubscribeRequestMethod>()))
    }

    fn unsubscribe(
        &self,
        _request: UnsubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), McpError>> + Send + '_ {
        std::future::ready(Err(McpError::method_not_found::<UnsubscribeRequestMethod>()))
    }

    fn on_cancelled(
        &self,
        _notification: CancelledNotificationParam,
    ) -> impl Future<Output = ()> + Send + '_ {
        std::future::ready(())
    }

    fn on_progress(
        &self,
        _notification: ProgressNotificationParam,
    ) -> impl Future<Output = ()> + Send + '_ {
        std::future::ready(())
    }

    fn on_initialized(&self) -> impl Future<Output = ()> + Send + '_ {
        std::future::ready(())
    }

    fn on_roots_list_changed(&self) -> impl Future<Output = ()> + Send + '_ {
        std::future::ready(())
    }

    fn get_peer(&self) -> Option<Peer<RoleServer>> {
        None
    }

    fn set_peer(&mut self, peer: Peer<RoleServer>) {
        drop(peer);
    }
}
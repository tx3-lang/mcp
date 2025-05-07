use std::fs;
use std::collections::HashMap;
use rmcp::{Error as McpError, ServerHandler, model::*, schemars, tool};
use tx3_sdk::trp::{Client as TrpClient, ClientOptions, ProtoTxRequest, TirInfo};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListTransactionsRequest {
    pub protocol_name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListParametersRequest {
    pub protocol_name: String,
    pub transaction_name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResolveRequest {
    pub protocol_name: String,
    pub transaction_name: String,
    pub parameters: HashMap<String, String>,
}

#[derive(Clone)]
pub struct Protocol {
    protocols: HashMap<String, String>,
    trp_url: String,
    trp_key: String,
}

#[tool(tool_box)]
impl Protocol {
    #[allow(dead_code)]
    pub fn new(trp_url: &str, trp_key: &str) -> Self {
        let mut protocols = HashMap::new();
        let paths = fs::read_dir("./protocols").unwrap();
        for path in paths {
            let path = path.unwrap();
            if path.file_name().to_str().unwrap().ends_with(".tx3") {
                let message = fs::read_to_string(path.path());
                if message.is_err() {
                    continue;
                }
                
                let name = path.file_name().to_str().unwrap().replace(".tx3", "");
                protocols.insert(name, message.ok().unwrap());
            }
        }

        Self {
            protocols: protocols,
            trp_url: trp_url.to_string(),
            trp_key: trp_key.to_string(),
        }
    }

    #[tool(description = "List all the available protocols")]
    fn list_protocols(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(
            self.protocols
                .keys()
                .map(|name| Content::text(name.clone()))
                .collect::<Vec<Content>>(),
        ))
    }

    #[tool(description = "Receives a protocol name and returns the list of transactions available for that protocol")]
    fn list_protocol_transactions(
        &self,
        #[tool(aggr)] ListTransactionsRequest { protocol_name }: ListTransactionsRequest
    ) -> Result<CallToolResult, McpError> {
        let protocol_string = self.protocols.get(&protocol_name).ok_or_else(|| {
            McpError::new(
                ErrorCode::RESOURCE_NOT_FOUND,
                format!("Protocol {} not found", protocol_name),
                None
            )
        })?;

        let protocol = tx3_lang::Protocol::from_string(protocol_string.to_string()).load().unwrap();

        Ok(CallToolResult::success(
            protocol
                .txs()
                .map(|tx| Content::text(tx.name.to_string()))
                .collect::<Vec<Content>>(),
        ))
    }

    #[tool(description = "Receives a protocol name and transaction name and returns the list of parameters required for resolving that transaction")]
    fn list_transaction_parameters(
        &self,
        #[tool(aggr)] ListParametersRequest { protocol_name, transaction_name }: ListParametersRequest
    ) -> Result<CallToolResult, McpError> {
        let protocol_string = self.protocols.get(&protocol_name).ok_or_else(|| {
            McpError::new(
                ErrorCode::RESOURCE_NOT_FOUND,
                format!("Protocol {} not found", protocol_name),
                None
            )
        })?;

        let protocol = tx3_lang::Protocol::from_string(protocol_string.to_string()).load().unwrap();

        let prototx = protocol.new_tx(transaction_name.as_str());
        if prototx.is_err() {
            return Err(McpError::new(
                ErrorCode::RESOURCE_NOT_FOUND,
                format!("Transaction {} not found for protocol {}", transaction_name, protocol_name),
                None
            ));
        }

        Ok(CallToolResult::success(
            prototx.unwrap().find_params().into_iter()
                .map(|param| Content::text(format!("{}: {:?}", param.0, param.1)))
                .collect::<Vec<Content>>(),
        ))
    }

    #[tool(description = "Receives a protocol name, transaction name and parameters and returns the resulting CBOR of that transaction")]
    async fn resolve_transaction(
        &self,
        #[tool(aggr)] ResolveRequest { protocol_name, transaction_name, parameters }: ResolveRequest
    ) -> Result<CallToolResult, McpError> {
        let protocol_string = self.protocols.get(&protocol_name).ok_or_else(|| {
            McpError::new(
                ErrorCode::RESOURCE_NOT_FOUND,
                format!("Protocol {} not found", protocol_name),
                None
            )
        })?;

        let prototx = {
            let protocol = tx3_lang::Protocol::from_string(protocol_string.to_string()).load().unwrap();
            let prototx_result = protocol.new_tx(transaction_name.as_str());
            if prototx_result.is_err() {
                return Err(McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Transaction {} not found for protocol {}", transaction_name, protocol_name),
                    None
                ));
            }
            prototx_result.unwrap()
        };

        let parameters_types = prototx.find_params();

        let mut args: HashMap<String, tx3_lang::ArgValue> = HashMap::new();
        for (arg_name, string_value) in parameters.iter() {
            let arg_type = parameters_types.get(arg_name).ok_or_else(|| {
                McpError::new(
                    ErrorCode::RESOURCE_NOT_FOUND,
                    format!("Parameter {} not found for transaction {}", arg_name, transaction_name),
                    None
                )
            })?;

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
                version: "v1alpha1".to_string(),
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
}

#[tool(tool_box)]
impl ServerHandler for Protocol {
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
}
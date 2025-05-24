use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumMessage {
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StratumError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum StratumRequest {
    Subscribe {
        id: u64,
        user_agent: Option<String>,
    },
    Authorize {
        id: u64,
        worker_name: String,
        password: Option<String>,
    },
    Submit {
        id: u64,
        worker_name: String,
        job_id: String,
        nonce: u64,
        share_data: ShareSubmissionData,
    },
    GetStatus {
        id: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareSubmissionData {
    ComputationProof {
        witness_commitment: [u8; 32],
        computation_steps: u64,
    },
    ValidBlock {
        proof: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub enum StratumResponse {
    Result {
        id: u64,
        result: Value,
    },
    Error {
        id: u64,
        error: StratumError,
    },
    Notification {
        method: String,
        params: Value,
    },
}

impl StratumMessage {
    pub fn parse_request(&self) -> Result<StratumRequest, StratumError> {
        let method = self.method.as_ref().ok_or_else(|| StratumError {
            code: -32600,
            message: "Invalid request: missing method".to_string(),
            data: None,
        })?;
        
        let id = self.id.ok_or_else(|| StratumError {
            code: -32600,
            message: "Invalid request: missing id".to_string(),
            data: None,
        })?;
        
        match method.as_str() {
            "mining.subscribe" => {
                let user_agent = self.params
                    .as_ref()
                    .and_then(|p| p.as_array())
                    .and_then(|a| a.get(0))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                
                Ok(StratumRequest::Subscribe { id, user_agent })
            }
            "mining.authorize" => {
                let params = self.params.as_ref()
                    .and_then(|p| p.as_array())
                    .ok_or_else(|| StratumError {
                        code: -32602,
                        message: "Invalid params".to_string(),
                        data: None,
                    })?;
                
                let worker_name = params.get(0)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| StratumError {
                        code: -32602,
                        message: "Missing worker name".to_string(),
                        data: None,
                    })?
                    .to_string();
                
                let password = params.get(1)
                    .and_then(|v| v.as_str())
                    .map(String::from);
                
                Ok(StratumRequest::Authorize { id, worker_name, password })
            }
            "mining.submit" => {
                let params = self.params.as_ref()
                    .ok_or_else(|| StratumError {
                        code: -32602,
                        message: "Invalid params".to_string(),
                        data: None,
                    })?;
                
                // Parse the submit parameters
                let submission: crate::shares::ShareSubmission = serde_json::from_value(params.clone())
                    .map_err(|_| StratumError {
                        code: -32602,
                        message: "Invalid submission format".to_string(),
                        data: None,
                    })?;
                
                Ok(StratumRequest::Submit {
                    id,
                    worker_name: submission.miner_id.clone(),
                    job_id: submission.job_id,
                    nonce: 0, // Will be extracted from share_data
                    share_data: match submission.share_type {
                        crate::shares::ShareType::ComputationProof { witness_commitment, computation_steps, .. } => {
                            ShareSubmissionData::ComputationProof { witness_commitment, computation_steps }
                        }
                        crate::shares::ShareType::ValidBlock { proof, .. } => {
                            ShareSubmissionData::ValidBlock { proof }
                        }
                    },
                })
            }
            "mining.get_status" => Ok(StratumRequest::GetStatus { id }),
            _ => Err(StratumError {
                code: -32601,
                message: format!("Unknown method: {}", method),
                data: None,
            }),
        }
    }
}

impl StratumResponse {
    pub fn to_message(&self) -> StratumMessage {
        match self {
            StratumResponse::Result { id, result } => StratumMessage {
                id: Some(*id),
                method: None,
                params: None,
                result: Some(result.clone()),
                error: None,
            },
            StratumResponse::Error { id, error } => StratumMessage {
                id: Some(*id),
                method: None,
                params: None,
                result: None,
                error: Some(error.clone()),
            },
            StratumResponse::Notification { method, params } => StratumMessage {
                id: None,
                method: Some(method.clone()),
                params: Some(params.clone()),
                result: None,
                error: None,
            },
        }
    }
}
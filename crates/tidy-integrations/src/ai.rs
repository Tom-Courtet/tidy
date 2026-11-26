//! Chat IA : trait [`AiProvider`] et API Claude (Messages API, réponse en flux SSE).
//!
//! La clé vient du gestionnaire d'identifiants ([`crate::secret`]) et ne quitte la
//! mémoire que vers `api.anthropic.com` (en-tête `x-api-key`). Ni la clé ni le texte de
//! la conversation ne vont dans les journaux.

use std::fmt;
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::net::{self, AgentOptions, NetError};

/// Modèle par défaut.
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
/// Plafond de la réponse : large, le flux évite les délais dépassés.
const MAX_TOKENS: u32 = 64_000;
/// Durée maximale d'une réponse complète.
const STREAM_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Modèles qui acceptent le repli côté serveur en cas de refus (`fallbacks`).
const FALLBACK_MODELS: &[&str] = &[
    "claude-sonnet-5-5",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-fable-5-1",
    "claude-fable-5-2",
];
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Auteur d'un message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// Message de la conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub text: String,
}

/// Demande au modèle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    /// Alternance utilisateur / assistant, en commençant et finissant par l'utilisateur.
    pub messages: Vec<ChatMessage>,
}

/// Fin de réponse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// Réponse complète.
    EndTurn,
    /// Coupée au plafond de longueur.
    MaxTokens,
    /// Le modèle a refusé de répondre.
    Refusal,
    Other(String),
}

/// Erreur du chat, sans texte d'interface : le widget la traduit.
#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    /// Aucune clé enregistrée.
    NoKey,
    /// Clé refusée (401).
    Auth,
    /// Clé sans droit sur ce modèle (403).
    Permission,
    /// Modèle inconnu (404).
    NotFound(String),
    /// Trop de demandes (429).
    RateLimited,
    /// Service surchargé (529).
    Overloaded,
    /// Demande refusée (400), avec le message de l'API.
    BadRequest(String),
    /// Erreur du service (5xx).
    Server(u16),
    Net(NetError),
    /// Flux coupé avant la fin : la réponse partielle reste affichée.
    Interrupted,
    /// Autre erreur annoncée par l'API.
    Api(String),
    /// Arrêtée par l'utilisateur.
    Cancelled,
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NoKey => write!(f, "aucune clé"),
            AiError::Auth => write!(f, "clé refusée"),
            AiError::Permission => write!(f, "accès refusé"),
            AiError::NotFound(m) => write!(f, "introuvable : {m}"),
            AiError::RateLimited => write!(f, "trop de demandes"),
            AiError::Overloaded => write!(f, "service surchargé"),
            AiError::BadRequest(m) => write!(f, "demande refusée : {m}"),
            AiError::Server(code) => write!(f, "erreur du service ({code})"),
            AiError::Net(e) => e.fmt(f),
            AiError::Interrupted => write!(f, "réponse coupée"),
            AiError::Api(m) => write!(f, "erreur de l'API : {m}"),
            AiError::Cancelled => write!(f, "arrêtée"),
        }
    }
}

impl std::error::Error for AiError {}

/// Fournisseur de chat.
pub trait AiProvider: Send + Sync {
    /// Envoie la conversation et passe chaque morceau de texte à `on_text`, au fil de
    /// l'eau. Bloquant : à appeler hors du fil de l'interface. `cancel` arrête le flux.
    fn stream(
        &self,
        request: &ChatRequest,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<StopReason, AiError>;
}

/// API Claude d'Anthropic.
pub struct Claude {
    api_key: String,
}

impl fmt::Debug for Claude {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Claude").field("api_key", &"…").finish()
    }
}

impl Claude {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

/// Corps JSON de la demande, et en-tête bêta éventuel.
pub fn request_body(request: &ChatRequest) -> (Value, Option<&'static str>) {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|m| {
            json!({
                "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": m.text,
            })
        })
        .collect();
    let mut body = json!({
        "model": request.model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "messages": messages,
    });
    if !request.system.trim().is_empty() {
        body["system"] = json!(request.system);
    }
    // Sur les modèles qui le permettent, un refus de sécurité est repris par un autre
    // modèle dans le même flux, au lieu de laisser la réponse vide.
    let beta = if FALLBACK_MODELS.contains(&request.model.as_str()) {
        body["fallbacks"] = json!("default");
        Some(FALLBACK_BETA)
    } else {
        None
    };
    (body, beta)
}

impl AiProvider for Claude {
    fn stream(
        &self,
        request: &ChatRequest,
        cancel: &AtomicBool,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<StopReason, AiError> {
        if self.api_key.trim().is_empty() {
            return Err(AiError::NoKey);
        }
        let (body, beta) = request_body(request);
        let agent = net::agent(AgentOptions {
            total: Some(STREAM_TIMEOUT),
            status_as_response: true,
        });
        let mut call = agent
            .post(API_URL)
            .header("x-api-key", self.api_key.trim())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream");
        if let Some(beta) = beta {
            call = call.header("anthropic-beta", beta);
        }
        let response = call
            .send(body.to_string())
            .map_err(|e| AiError::Net(e.into()))?;
        let status = response.status().as_u16();
        let mut body = response.into_body();
        if status != 200 {
            let text = body
                .with_config()
                .limit(64 * 1024)
                .read_to_string()
                .unwrap_or_default();
            let err = error_from_status(status, &text);
            log::warn!("API Claude : HTTP {status} ({err})");
            return Err(err);
        }
        let reader = std::io::BufReader::new(body.into_reader());
        read_stream(reader, cancel, on_text)
    }
}

/// Message d'erreur d'un corps `{"type":"error","error":{"type":…,"message":…}}`.
fn api_error(body: &str) -> (String, String) {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let err = &v["error"];
    (
        err["type"].as_str().unwrap_or_default().to_owned(),
        err["message"].as_str().unwrap_or_default().to_owned(),
    )
}

/// Erreur d'après le statut HTTP et le corps de la réponse.
pub fn error_from_status(status: u16, body: &str) -> AiError {
    let (kind, message) = api_error(body);
    match status {
        401 => AiError::Auth,
        403 => AiError::Permission,
        404 => AiError::NotFound(message),
        429 => AiError::RateLimited,
        529 => AiError::Overloaded,
        400 | 413 | 422 => AiError::BadRequest(message),
        500..=599 => AiError::Server(status),
        _ => error_from_kind(&kind, message),
    }
}

/// Erreur d'après son type (`overloaded_error`…), dans un flux ou un corps.
fn error_from_kind(kind: &str, message: String) -> AiError {
    match kind {
        "authentication_error" => AiError::Auth,
        "permission_error" => AiError::Permission,
        "not_found_error" => AiError::NotFound(message),
        "rate_limit_error" => AiError::RateLimited,
        "overloaded_error" => AiError::Overloaded,
        "invalid_request_error" | "request_too_large" => AiError::BadRequest(message),
        "api_error" => AiError::Server(500),
        _ => AiError::Api(if message.is_empty() {
            kind.to_owned()
        } else {
            message
        }),
    }
}

/// Lit un flux SSE de la Messages API jusqu'à `message_stop`. Les blocs autres que du
/// texte (réflexion, repli de modèle) sont ignorés.
pub fn read_stream(
    mut reader: impl BufRead,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
) -> Result<StopReason, AiError> {
    let mut line = String::new();
    let mut data = String::new();
    let mut stop = StopReason::EndTurn;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err(AiError::Interrupted),
            Ok(_) => {}
            Err(e) => {
                log::warn!("flux Claude coupé : {}", net::io_error(&e));
                return Err(AiError::Interrupted);
            }
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if data.is_empty() {
                continue;
            }
            let event: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            data.clear();
            match event["type"].as_str().unwrap_or_default() {
                "content_block_delta" => {
                    let delta = &event["delta"];
                    if delta["type"] == "text_delta"
                        && let Some(text) = delta["text"].as_str()
                    {
                        on_text(text);
                    }
                }
                "message_delta" => {
                    if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                        stop = match reason {
                            "end_turn" | "stop_sequence" => StopReason::EndTurn,
                            "max_tokens" => StopReason::MaxTokens,
                            "refusal" => StopReason::Refusal,
                            other => StopReason::Other(other.to_owned()),
                        };
                    }
                }
                "message_stop" => return Ok(stop),
                "error" => {
                    let err = &event["error"];
                    let kind = err["type"].as_str().unwrap_or_default();
                    let message = err["message"].as_str().unwrap_or_default().to_owned();
                    log::warn!("flux Claude : erreur {kind}");
                    return Err(error_from_kind(kind, message));
                }
                _ => {}
            }
        } else if let Some(rest) = l.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
        // `event:`, `id:`, `retry:` et les commentaires (`:`) ne servent pas : le type
        // est aussi dans les données.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = include_str!("../tests/data/claude-stream-ok.sse");
    const ERROR: &str = include_str!("../tests/data/claude-stream-error.sse");
    const CUT: &str = include_str!("../tests/data/claude-stream-cut.sse");
    const REFUSAL: &str = include_str!("../tests/data/claude-stream-refusal.sse");
    const AUTH: &str = include_str!("../tests/data/claude-error-401.json");
    const BAD_MODEL: &str = include_str!("../tests/data/claude-error-404.json");

    fn run(sse: &str) -> (String, Result<StopReason, AiError>) {
        let cancel = AtomicBool::new(false);
        let mut text = String::new();
        let r = read_stream(sse.as_bytes(), &cancel, &mut |t| text.push_str(t));
        (text, r)
    }

    #[test]
    fn complete_stream_gives_text_and_ignores_thinking() {
        let (text, r) = run(OK);
        assert_eq!(r, Ok(StopReason::EndTurn));
        assert_eq!(text, "Bonjour ! Voici la traduction :\n\n« Hello, world ».");
    }

    #[test]
    fn error_event_in_the_stream_keeps_the_partial_text() {
        let (text, r) = run(ERROR);
        assert_eq!(text, "Le début de la");
        assert_eq!(r, Err(AiError::Overloaded));
    }

    #[test]
    fn cut_stream_is_reported_as_interrupted() {
        let (text, r) = run(CUT);
        assert_eq!(text, "Une réponse qui s'arr");
        assert_eq!(r, Err(AiError::Interrupted));
    }

    #[test]
    fn refusal_is_reported() {
        let (_, r) = run(REFUSAL);
        assert_eq!(r, Ok(StopReason::Refusal));
    }

    #[test]
    fn cancel_stops_reading() {
        let cancel = AtomicBool::new(true);
        let r = read_stream(OK.as_bytes(), &cancel, &mut |_| {});
        assert_eq!(r, Err(AiError::Cancelled));
    }

    #[test]
    fn http_errors_are_mapped() {
        assert_eq!(error_from_status(401, AUTH), AiError::Auth);
        assert_eq!(
            error_from_status(404, BAD_MODEL),
            AiError::NotFound("model: claude-inconnu".into())
        );
        assert_eq!(error_from_status(529, ""), AiError::Overloaded);
        assert_eq!(error_from_status(503, "<html>"), AiError::Server(503));
        assert!(matches!(
            error_from_status(400, "{}"),
            AiError::BadRequest(_)
        ));
    }

    #[test]
    fn request_body_has_the_conversation_and_fallbacks() {
        let req = ChatRequest {
            model: DEFAULT_MODEL.into(),
            system: "Sois bref.".into(),
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    text: "Salut".into(),
                },
                ChatMessage {
                    role: Role::Assistant,
                    text: "Bonjour".into(),
                },
                ChatMessage {
                    role: Role::User,
                    text: "Traduis « chat »".into(),
                },
            ],
        };
        let (body, beta) = request_body(&req);
        assert_eq!(body["stream"], true);
        assert_eq!(body["model"], "claude-sonnet-5-5");
        assert_eq!(body["system"], "Sois bref.");
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(body["messages"][2]["content"], "Traduis « chat »");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(beta, Some(FALLBACK_BETA));
        // Pas de réglage d'échantillonnage (refusé par les modèles récents).
        assert!(body.get("temperature").is_none());

        let old = ChatRequest {
            model: "claude-haiku-4-5".into(),
            system: String::new(),
            ..req
        };
        let (body, beta) = request_body(&old);
        assert!(body.get("fallbacks").is_none() && body.get("system").is_none());
        assert_eq!(beta, None);
    }

    #[test]
    fn debug_hides_the_key() {
        let shown = format!("{:?}", Claude::new("sk-ant-secret"));
        assert!(!shown.contains("secret"));
    }
}

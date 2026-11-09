//! Testes da sessão (config injetado) e dos handlers `retroachievements:*`.
//! A rede é sempre um wiremock local.

use std::time::Duration;

use arcadia_ra::{
    MemoryConfigStorage, RaConfigStorage, RaError, RaSession, RetroAchievementsClient, KEY_TOKEN,
    KEY_USERNAME, KEY_WEB_API_KEY,
};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const LOGIN_SUCCESS: &str = include_str!("fixtures/login_success.json");
const LOGIN_INVALID: &str = include_str!("fixtures/login_invalid.json");
const USER_SUMMARY: &str = include_str!("fixtures/user_summary.json");
const GAME_LIST: &str = include_str!("fixtures/game_list.json");
const GAME_PROGRESS: &str = include_str!("fixtures/game_progress.json");

fn client(server: &MockServer) -> RetroAchievementsClient {
    RetroAchievementsClient::builder()
        .host(server.uri())
        .timeout(Duration::from_secs(2))
        .build()
}

fn json_response(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "application/json")
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .expect("servidor wiremock ativo")
}

fn query_pairs(request: &Request) -> Vec<(String, String)> {
    request
        .url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[tokio::test]
async fn login_persiste_somente_usuario_e_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/dorequest.php"))
        .respond_with(json_response(LOGIN_SUCCESS))
        .mount(&server)
        .await;

    let storage = MemoryConfigStorage::new(json!({
        "profile": { "name": "Zes" },
        "unknown_future_key": true,
        "retroachievements_web_api_key": "chave-antiga"
    }));
    let session = RaSession::new(client(&server), storage.clone());

    let result = session.login("  Fulano  ", "senha-secreta").await.unwrap();
    assert_eq!(result.username, "Fulano");
    assert_eq!(result.token, "tok-123");

    let config = storage.snapshot();
    assert_eq!(config[KEY_USERNAME], "Fulano");
    assert_eq!(config[KEY_TOKEN], "tok-123");
    assert_eq!(config[KEY_WEB_API_KEY], "chave-antiga");
    assert_eq!(config["profile"]["name"], "Zes");
    assert_eq!(config["unknown_future_key"], true);

    // A senha nunca aparece em nenhum lugar do estado persistido.
    let serialized = serde_json::to_string(&config).unwrap();
    assert!(!serialized.contains("senha-secreta"));
    assert!(!config
        .as_object()
        .expect("config objeto")
        .keys()
        .any(|key| key.contains("password") || key.contains("senha")));

    let status = session.status();
    assert!(status.connected);
    assert_eq!(status.username, "Fulano");
    assert_eq!(
        status.to_value(),
        json!({ "ok": true, "connected": true, "username": "Fulano" })
    );

    let credentials = session.credentials().expect("credenciais");
    assert_eq!(credentials.username, "Fulano");
    assert_eq!(credentials.token, "tok-123");
    assert!(!format!("{credentials:?}").contains("tok-123"));
}

#[tokio::test]
async fn token_persistido_e_reutilizado_sem_novo_login() {
    let server = MockServer::start().await;
    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_TOKEN: "tok-persistido"
    }));
    let session = RaSession::new(client(&server), storage);

    let status = session.status();
    assert!(status.connected);
    assert_eq!(status.username, "Fulano");
    assert_eq!(
        session.credentials(),
        Some(arcadia_ra::RaCredentials {
            username: "Fulano".to_string(),
            token: "tok-persistido".to_string(),
        })
    );
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn login_falho_nao_toca_no_config() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/dorequest.php"))
        .respond_with(ResponseTemplate::new(401).set_body_raw(LOGIN_INVALID, "application/json"))
        .mount(&server)
        .await;

    let initial = json!({
        KEY_USERNAME: "Antigo",
        KEY_TOKEN: "token-antigo",
        KEY_WEB_API_KEY: "chave"
    });
    let storage = MemoryConfigStorage::new(initial.clone());
    let session = RaSession::new(client(&server), storage.clone());

    let error = session.login("Fulano", "errada").await.unwrap_err();
    assert_eq!(
        error,
        RaError::Server("Invalid user/password combination. Please try again.".to_string())
    );
    assert_eq!(storage.snapshot(), initial);
    assert_eq!(session.status().username, "Antigo");
}

#[tokio::test]
async fn logout_zera_usuario_e_token_e_preserva_web_api_key() {
    let server = MockServer::start().await;
    let storage = MemoryConfigStorage::new(json!({
        "language": "pt-BR",
        KEY_USERNAME: "Fulano",
        KEY_TOKEN: "tok-123",
        KEY_WEB_API_KEY: "chave"
    }));
    let session = RaSession::new(client(&server), storage.clone());

    session.logout().unwrap();

    let config = storage.snapshot();
    assert_eq!(config[KEY_USERNAME], "");
    assert_eq!(config[KEY_TOKEN], "");
    // O oráculo (main.js) não apaga a Web API Key no logout.
    assert_eq!(config[KEY_WEB_API_KEY], "chave");
    assert_eq!(config["language"], "pt-BR");
    assert!(!session.status().connected);
    assert_eq!(session.status().username, "");
    assert!(session.credentials().is_none());
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn troca_de_conta_substitui_usuario_e_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/dorequest.php"))
        .respond_with(json_response(
            r#"{"Success":true,"User":"Ciclano","Token":"tok-novo","Score":0}"#,
        ))
        .mount(&server)
        .await;

    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_TOKEN: "tok-velho",
        KEY_WEB_API_KEY: "chave-do-fulano"
    }));
    let session = RaSession::new(client(&server), storage.clone());

    session.login("Ciclano", "senha").await.unwrap();
    let config = storage.snapshot();
    assert_eq!(config[KEY_USERNAME], "Ciclano");
    assert_eq!(config[KEY_TOKEN], "tok-novo");
    assert!(!serde_json::to_string(&config)
        .unwrap()
        .contains("tok-velho"));
    // Mesmo comportamento do Electron: trocar de conta não limpa a chave.
    assert_eq!(config[KEY_WEB_API_KEY], "chave-do-fulano");
}

#[tokio::test]
async fn set_api_key_sem_usuario_devolve_nao_conectado() {
    let server = MockServer::start().await;
    let storage = MemoryConfigStorage::new(json!({}));
    let session = RaSession::new(client(&server), storage.clone());

    let error = session.set_api_key("chave").await.unwrap_err();
    assert_eq!(error, RaError::NotConnected);
    assert_eq!(error.code(), "nao_conectado");
    assert!(storage.snapshot().get(KEY_WEB_API_KEY).is_none());
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn set_api_key_valida_e_grava_com_trim() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetUserSummary.php"))
        .respond_with(json_response(USER_SUMMARY))
        .mount(&server)
        .await;

    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_TOKEN: "tok-123"
    }));
    let session = RaSession::new(client(&server), storage.clone());

    session.set_api_key("  chave \n").await.unwrap();

    let config = storage.snapshot();
    assert_eq!(config[KEY_WEB_API_KEY], "chave");
    assert_eq!(config[KEY_USERNAME], "Fulano");
    assert_eq!(config[KEY_TOKEN], "tok-123");

    let captured = requests(&server).await;
    assert_eq!(captured.len(), 1);
    assert_eq!(
        query_pairs(&captured[0]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
        ]
    );
}

#[tokio::test]
async fn set_api_key_invalida_nao_grava() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetUserSummary.php"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_raw(r#"{"message":"Unauthenticated."}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_TOKEN: "tok-123"
    }));
    let session = RaSession::new(client(&server), storage.clone());

    let error = session.set_api_key("invalida").await.unwrap_err();
    assert_eq!(error, RaError::Server("Unauthenticated.".to_string()));
    assert!(storage.snapshot().get(KEY_WEB_API_KEY).is_none());
}

#[tokio::test]
async fn set_api_key_vazia_falha_antes_da_rede() {
    let server = MockServer::start().await;
    let storage = MemoryConfigStorage::new(json!({ KEY_USERNAME: "Fulano" }));
    let session = RaSession::new(client(&server), storage);

    assert_eq!(
        session.set_api_key("   ").await.unwrap_err(),
        RaError::EmptyCredentials
    );
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn game_progress_sem_web_api_key() {
    let server = MockServer::start().await;
    let storage = MemoryConfigStorage::new(json!({ KEY_USERNAME: "Fulano" }));
    let session = RaSession::new(client(&server), storage);

    let error = session
        .game_progress("GTA", "sony-playstation-2")
        .await
        .unwrap_err();
    assert_eq!(error, RaError::MissingApiKey);
    assert_eq!(error.code(), "sem_web_api_key");
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn game_progress_sistema_sem_console_ra() {
    let server = MockServer::start().await;
    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_WEB_API_KEY: "chave"
    }));
    let session = RaSession::new(client(&server), storage);

    let error = session
        .game_progress("Jogo", "sony-playstation-3")
        .await
        .unwrap_err();
    assert_eq!(error, RaError::UnsupportedSystem);
    assert_eq!(error.code(), "sistema_nao_suportado");
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn game_progress_jogo_nao_encontrado_devolve_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetGameList.php"))
        .respond_with(json_response(GAME_LIST))
        .mount(&server)
        .await;

    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_WEB_API_KEY: "chave"
    }));
    let session = RaSession::new(client(&server), storage);

    let result = session
        .game_progress("Jogo Inexistente", "sony-playstation-2")
        .await
        .unwrap();
    assert_eq!(result, None);
    assert_eq!(requests(&server).await.len(), 1);
}

#[tokio::test]
async fn game_progress_resolve_id_e_normaliza() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetGameList.php"))
        .respond_with(json_response(GAME_LIST))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetGameInfoAndUserProgress.php"))
        .respond_with(json_response(GAME_PROGRESS))
        .mount(&server)
        .await;

    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_TOKEN: "tok-123",
        KEY_WEB_API_KEY: "chave"
    }));
    let session = RaSession::new(client(&server), storage);

    let progress = session
        .game_progress("Grand Theft Auto: San Andreas", "sony-playstation-2")
        .await
        .unwrap()
        .expect("jogo encontrado");
    assert_eq!(progress.game.id, 21);
    assert_eq!(progress.game.title, "GTA: SA");
    assert_eq!(progress.earned, 2);
    assert_eq!(progress.total, 3);
    assert_eq!(progress.completion_percentage, 66.67);

    let captured = requests(&server).await;
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].url.path(), "/API/API_GetGameList.php");
    assert_eq!(
        query_pairs(&captured[0]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
            ("i".to_string(), "21".to_string()),
        ]
    );
    assert_eq!(
        captured[1].url.path(),
        "/API/API_GetGameInfoAndUserProgress.php"
    );
    assert_eq!(
        query_pairs(&captured[1]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
            ("g".to_string(), "21".to_string()),
        ]
    );
}

#[tokio::test]
async fn game_progress_propaga_erro_da_busca() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetGameList.php"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_raw(r#"{"message":"Unauthenticated."}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let storage = MemoryConfigStorage::new(json!({
        KEY_USERNAME: "Fulano",
        KEY_WEB_API_KEY: "invalida"
    }));
    let session = RaSession::new(client(&server), storage);

    let error = session
        .game_progress("GTA", "sony-playstation-2")
        .await
        .unwrap_err();
    assert_eq!(error, RaError::Server("Unauthenticated.".to_string()));
}

struct FailingStorage;

impl RaConfigStorage for FailingStorage {
    fn read_config(&self) -> Value {
        json!({})
    }

    fn write_config_patch(&self, _patch: &Value) -> Result<Value, String> {
        Err("disco cheio".to_string())
    }
}

#[tokio::test]
async fn falha_do_storage_vira_erro_sem_vazar_a_senha() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/dorequest.php"))
        .respond_with(json_response(LOGIN_SUCCESS))
        .mount(&server)
        .await;

    let session = RaSession::new(client(&server), FailingStorage);
    let error = session.login("Fulano", "senha-secreta").await.unwrap_err();
    assert_eq!(error, RaError::Storage("disco cheio".to_string()));
    assert!(!format!("{error:?}").contains("senha-secreta"));
    assert!(!error.to_string().contains("senha-secreta"));
}

#[tokio::test]
async fn storage_em_memoria_preserva_chaves_desconhecidas_no_merge() {
    let storage = MemoryConfigStorage::new(json!({
        "unknown": { "nested": 1 },
        KEY_USERNAME: "Antigo"
    }));
    let next = storage
        .write_config_patch(&json!({ KEY_USERNAME: "Novo" }))
        .unwrap();
    assert_eq!(next["unknown"]["nested"], 1);
    assert_eq!(storage.snapshot()[KEY_USERNAME], "Novo");
}

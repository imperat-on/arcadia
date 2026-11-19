//! Testes do cliente HTTP contra um servidor local (wiremock). Nenhuma
//! requisição sai da máquina.

use std::time::Duration;

use arcadia_ra::{
    resolve_game_id, GameRef, RaError, RetroAchievementsClient, DEFAULT_HOST, GAME_LIST_PATH,
    GAME_PROGRESS_PATH, LOGIN_PATH, USER_AGENT, USER_SUMMARY_PATH,
};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
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

fn body_text(request: &Request) -> String {
    String::from_utf8(request.body.clone()).expect("corpo UTF-8")
}

#[test]
fn constantes_iguais_ao_oraculo() {
    assert_eq!(DEFAULT_HOST, "https://retroachievements.org");
    assert_eq!(USER_AGENT, "Arcadia/1.0 (Electron)");
    assert_eq!(LOGIN_PATH, "/dorequest.php");
    assert_eq!(USER_SUMMARY_PATH, "/API/API_GetUserSummary.php");
    assert_eq!(GAME_LIST_PATH, "/API/API_GetGameList.php");
    assert_eq!(
        GAME_PROGRESS_PATH,
        "/API/API_GetGameInfoAndUserProgress.php"
    );
}

#[tokio::test]
async fn login_sucesso_envia_form_exato_e_devolve_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .and(header("user-agent", USER_AGENT))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .respond_with(json_response(LOGIN_SUCCESS))
        .mount(&server)
        .await;

    let result = client(&server)
        .login("Fulano", "senha")
        .await
        .expect("login");

    assert_eq!(result.username, "Fulano");
    assert_eq!(result.token, "tok-123");
    assert_eq!(result.score, 42);

    let captured = requests(&server).await;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method.as_str(), "POST");
    assert_eq!(captured[0].url.path(), LOGIN_PATH);
    assert!(captured[0].url.query().is_none());
    assert_eq!(body_text(&captured[0]), "r=login2&u=Fulano&p=senha");
    assert_eq!(
        captured[0]
            .headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok()),
        Some(USER_AGENT)
    );
}

#[tokio::test]
async fn login_codifica_espacos_e_caracteres_especiais_como_urlsearchparams() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(json_response(LOGIN_SUCCESS))
        .mount(&server)
        .await;

    client(&server)
        .login("  Fu lano  ", "p@ss&w=rd")
        .await
        .expect("login");

    let captured = requests(&server).await;
    assert_eq!(
        body_text(&captured[0]),
        "r=login2&u=Fu+lano&p=p%40ss%26w%3Drd"
    );
}

#[tokio::test]
async fn login_401_le_o_error_real_do_corpo() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(401).set_body_raw(LOGIN_INVALID, "application/json"))
        .mount(&server)
        .await;

    let error = client(&server).login("Fulano", "errada").await.unwrap_err();
    assert_eq!(
        error,
        RaError::Server("Invalid user/password combination. Please try again.".to_string())
    );
}

#[tokio::test]
async fn login_sem_error_usa_credenciais_invalidas() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(json_response(r#"{"Success":false}"#))
        .mount(&server)
        .await;

    let error = client(&server).login("Fulano", "x").await.unwrap_err();
    assert_eq!(error, RaError::InvalidCredentials);
    assert_eq!(error.code(), "credenciais_invalidas");
}

#[tokio::test]
async fn login_resposta_nao_json_vira_resposta_invalida() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<html>not json</html>", "text/html"))
        .mount(&server)
        .await;

    let error = client(&server).login("a", "b").await.unwrap_err();
    assert_eq!(error, RaError::InvalidResponse);
    assert_eq!(error.code(), "resposta_invalida");
}

#[tokio::test]
async fn login_credenciais_vazias_nao_chamam_a_rede() {
    let server = MockServer::start().await;
    let error = client(&server).login("   ", "x").await.unwrap_err();
    assert_eq!(error, RaError::EmptyCredentials);
    assert_eq!(error.code(), "credenciais_vazias");
    assert!(requests(&server).await.is_empty());

    let error = client(&server).login("Fulano", "").await.unwrap_err();
    assert_eq!(error, RaError::EmptyCredentials);
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn login_tempo_esgotado() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(json_response(LOGIN_SUCCESS).set_delay(Duration::from_millis(500)))
        .mount(&server)
        .await;

    let client = RetroAchievementsClient::builder()
        .host(server.uri())
        .timeout(Duration::from_millis(40))
        .build();
    let error = client.login("Fulano", "senha").await.unwrap_err();
    assert_eq!(error, RaError::Timeout);
    assert_eq!(error.code(), "tempo_esgotado");
}

#[tokio::test]
async fn login_falha_de_rede() {
    // Servidor TCP local que aceita a conexão e fecha sem responder: o hyper
    // devolve erro de transporte (falha_rede), sem rede externa.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("porta local");
    let port = listener.local_addr().expect("endereço local").port();
    let server = std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            drop(stream);
        }
    });
    let host = format!("http://127.0.0.1:{port}");

    let client = RetroAchievementsClient::builder()
        .host(host)
        .timeout(Duration::from_secs(2))
        .build();
    let error = client.login("Fulano", "senha").await.unwrap_err();
    server.join().expect("thread do servidor local");
    assert_eq!(error, RaError::Network);
    assert_eq!(error.code(), "falha_rede");
}

#[tokio::test]
async fn verify_api_key_sucesso_com_query_ordenada() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(USER_SUMMARY_PATH))
        .and(header("user-agent", USER_AGENT))
        .respond_with(json_response(USER_SUMMARY))
        .mount(&server)
        .await;

    let result = client(&server)
        .verify_api_key("Fulano", "chave")
        .await
        .expect("verify");
    assert_eq!(result.username, "Fulano");

    let captured = requests(&server).await;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].url.path(), USER_SUMMARY_PATH);
    assert_eq!(
        query_pairs(&captured[0]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
        ]
    );
    assert_eq!(captured[0].url.query(), Some("u=Fulano&y=chave"));
}

#[tokio::test]
async fn verify_api_key_usa_usuario_da_entrada_quando_user_vem_vazio() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(USER_SUMMARY_PATH))
        .respond_with(json_response(r#"{"User":""}"#))
        .mount(&server)
        .await;

    let result = client(&server)
        .verify_api_key(" Fulano ", "chave")
        .await
        .expect("verify");
    assert_eq!(result.username, "Fulano");
}

#[tokio::test]
async fn verify_api_key_propaga_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(USER_SUMMARY_PATH))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_raw(r#"{"Error":"Invalid API Key"}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let error = client(&server)
        .verify_api_key("Fulano", "invalida")
        .await
        .unwrap_err();
    assert_eq!(error, RaError::Server("Invalid API Key".to_string()));
}

#[tokio::test]
async fn verify_api_key_propaga_message_do_middleware() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(USER_SUMMARY_PATH))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_raw(r#"{"message":"Unauthenticated."}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let error = client(&server)
        .verify_api_key("Fulano", "invalida")
        .await
        .unwrap_err();
    assert_eq!(error, RaError::Server("Unauthenticated.".to_string()));
}

#[tokio::test]
async fn verify_api_key_200_com_error_continua_sucesso_como_no_js() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(USER_SUMMARY_PATH))
        .respond_with(json_response(r#"{"Error":"algo"}"#))
        .mount(&server)
        .await;

    let result = client(&server)
        .verify_api_key("Fulano", "chave")
        .await
        .expect("o JS só olha o corpo quando !response.ok");
    assert_eq!(result.username, "Fulano");
}

#[tokio::test]
async fn verify_api_key_credenciais_vazias_nao_chamam_a_rede() {
    let server = MockServer::start().await;
    assert_eq!(
        client(&server)
            .verify_api_key("", "chave")
            .await
            .unwrap_err(),
        RaError::EmptyCredentials
    );
    assert_eq!(
        client(&server)
            .verify_api_key("Fulano", "  ")
            .await
            .unwrap_err(),
        RaError::EmptyCredentials
    );
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn find_game_by_title_exato_prefere_titulo_exato() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(json_response(GAME_LIST))
        .mount(&server)
        .await;

    let game = client(&server)
        .find_game_by_title("Fulano", "chave", "Grand Theft Auto: San Andreas", 21)
        .await
        .expect("busca");
    assert_eq!(
        game,
        Some(GameRef {
            id: 21,
            title: "Grand Theft Auto: San Andreas".to_string()
        })
    );
    let captured = requests(&server).await;
    assert_eq!(
        query_pairs(&captured[0]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
            ("i".to_string(), "21".to_string()),
        ]
    );
}

#[tokio::test]
async fn find_game_by_title_aceita_correspondencia_parcial_case_insensitive() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(json_response(GAME_LIST))
        .mount(&server)
        .await;

    let game = client(&server)
        .find_game_by_title("Fulano", "chave", "san andreas", 21)
        .await
        .expect("busca");
    assert_eq!(game.map(|game| game.id), Some(21));
}

#[tokio::test]
async fn find_game_by_title_exato_ganha_de_parcial_anterior() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(json_response(
            r#"[
                {"ID": 1, "Title": "Grand Theft Auto: San Andreas - Special Edition"},
                {"ID": 2, "Title": "Grand Theft Auto: San Andreas"}
            ]"#,
        ))
        .mount(&server)
        .await;

    let game = client(&server)
        .find_game_by_title("Fulano", "chave", "Grand Theft Auto: San Andreas", 21)
        .await
        .expect("busca");
    assert_eq!(game.map(|game| game.id), Some(2));
}

#[tokio::test]
async fn find_game_by_title_sem_correspondencia_devolve_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(json_response(GAME_LIST))
        .mount(&server)
        .await;

    let game = client(&server)
        .find_game_by_title("Fulano", "chave", "Jogo Inexistente", 21)
        .await
        .expect("busca");
    assert_eq!(game, None);
}

#[tokio::test]
async fn find_game_by_title_resposta_que_nao_e_array_vira_falha_busca() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(json_response(r#"{"jogos":[]}"#))
        .mount(&server)
        .await;

    let error = client(&server)
        .find_game_by_title("Fulano", "chave", "X", 21)
        .await
        .unwrap_err();
    assert_eq!(error, RaError::SearchFailed);
}

#[tokio::test]
async fn find_game_by_title_propaga_erro_do_servidor() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_raw(r#"{"message":"Unauthenticated."}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let error = client(&server)
        .find_game_by_title("Fulano", "invalida", "X", 21)
        .await
        .unwrap_err();
    assert_eq!(error, RaError::Server("Unauthenticated.".to_string()));
}

#[tokio::test]
async fn find_game_by_title_parametros_invalidos_nao_chamam_a_rede() {
    let server = MockServer::start().await;
    let client = client(&server);
    assert_eq!(
        client
            .find_game_by_title("", "chave", "X", 21)
            .await
            .unwrap_err(),
        RaError::InvalidParameters
    );
    assert_eq!(
        client
            .find_game_by_title("Fulano", "", "X", 21)
            .await
            .unwrap_err(),
        RaError::InvalidParameters
    );
    assert_eq!(
        client
            .find_game_by_title("Fulano", "chave", "   ", 21)
            .await
            .unwrap_err(),
        RaError::InvalidParameters
    );
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn find_game_by_title_console_zero_vira_query_i_vazia() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(json_response("[]"))
        .mount(&server)
        .await;

    let game = client(&server)
        .find_game_by_title("Fulano", "chave", "X", 0)
        .await
        .expect("busca");
    assert_eq!(game, None);
    assert_eq!(
        query_pairs(&requests(&server).await[0]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
            ("i".to_string(), String::new()),
        ]
    );
}

#[tokio::test]
async fn get_game_progress_normaliza_achievements_e_datas() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_PROGRESS_PATH))
        .and(header("user-agent", USER_AGENT))
        .respond_with(json_response(GAME_PROGRESS))
        .mount(&server)
        .await;

    let progress = client(&server)
        .get_game_progress("Fulano", "chave", 21)
        .await
        .expect("progresso");

    assert_eq!(progress.game.id, 21);
    assert_eq!(progress.game.title, "GTA: SA");
    assert_eq!(progress.game.console_name, "PlayStation 2");
    assert_eq!(progress.game.num_achievements, 3);
    assert_eq!(progress.game.num_awarded_to_user, 2);
    assert_eq!(progress.game.num_awarded_to_user_hardcore, 1);
    assert_eq!(progress.earned, 2);
    assert_eq!(progress.total, 3);
    assert_eq!(progress.completion_percentage, 66.67);
    assert_eq!(progress.hardcore_earned, 1);
    assert_eq!(progress.hardcore_completion_percentage, 33.33);
    assert_eq!(progress.achievements.len(), 3);

    let first = &progress.achievements[0];
    assert_eq!(first.id, 9);
    assert_eq!(first.points, 10);
    assert!(first.unlocked);
    assert!(first.unlocked_hardcore);
    assert_eq!(
        first.badge_url,
        "https://media.retroachievements.org/Badge/325108.png"
    );
    assert_eq!(
        first.badge_locked_url,
        "https://media.retroachievements.org/Badge/325108_lock.png"
    );
    assert_eq!(first.date_earned.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert_eq!(
        first.date_earned_hardcore.as_deref(),
        Some("2024-01-02T03:04:05Z")
    );
    assert_eq!(
        progress.achievements[1].date_earned.as_deref(),
        Some("2024-02-29T23:59:59Z")
    );
    assert!(!progress.achievements[2].unlocked);
    assert_eq!(progress.achievements[2].badge_url, "");

    let captured = requests(&server).await;
    assert_eq!(
        query_pairs(&captured[0]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
            ("g".to_string(), "21".to_string()),
        ]
    );
}

#[tokio::test]
async fn get_game_progress_id_invalido_nao_chama_a_rede() {
    let server = MockServer::start().await;
    for game_id in [0, -1] {
        assert_eq!(
            client(&server)
                .get_game_progress("Fulano", "chave", game_id)
                .await
                .unwrap_err(),
            RaError::InvalidParameters
        );
    }
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn get_game_progress_propaga_erro_do_servidor() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_PROGRESS_PATH))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_raw(r#"{"Error":"Internal error"}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let error = client(&server)
        .get_game_progress("Fulano", "chave", 21)
        .await
        .unwrap_err();
    assert_eq!(error, RaError::Server("Internal error".to_string()));
}

#[tokio::test]
async fn get_game_progress_resposta_nao_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_PROGRESS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string("nope"))
        .mount(&server)
        .await;

    let error = client(&server)
        .get_game_progress("Fulano", "chave", 21)
        .await
        .unwrap_err();
    assert_eq!(error, RaError::InvalidResponse);
}

#[tokio::test]
async fn resolve_game_id_usa_console_do_sistema() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(GAME_LIST_PATH))
        .respond_with(json_response(GAME_LIST))
        .mount(&server)
        .await;

    let id = resolve_game_id(
        &client(&server),
        "Fulano",
        "chave",
        "Grand Theft Auto: San Andreas",
        "sony-playstation-2",
    )
    .await
    .expect("resolver id");
    assert_eq!(id, Some(21));
    assert_eq!(
        query_pairs(&requests(&server).await[0]),
        vec![
            ("u".to_string(), "Fulano".to_string()),
            ("y".to_string(), "chave".to_string()),
            ("i".to_string(), "21".to_string()),
        ]
    );
}

#[tokio::test]
async fn resolve_game_id_recusa_sistema_sem_console_ra() {
    let server = MockServer::start().await;
    let error = resolve_game_id(
        &client(&server),
        "Fulano",
        "chave",
        "Jogo",
        "sony-playstation-3",
    )
    .await
    .unwrap_err();
    assert_eq!(error, RaError::UnsupportedSystem);
    assert!(requests(&server).await.is_empty());
}

#[test]
fn erros_montam_envelope_do_ipc() {
    let value: Value = RaError::MissingApiKey.to_value();
    assert_eq!(value, json!({ "ok": false, "error": "sem_web_api_key" }));
    let value = RaError::Server("Invalid API Key".to_string()).to_value();
    assert_eq!(value, json!({ "ok": false, "error": "Invalid API Key" }));
}

use axum::{
    routing::{get, post, IntoMakeService},
    Extension, Form, Json, Router, TypedHeader,
};
use ethers_core::abi::Address;
use ethers_signers::{LocalWallet, Signer};
use headers::{authorization::Bearer, Authorization};
use http::StatusCode;
use hyper::{server::conn::AddrIncoming, Server};
use kzg_ceremony_crypto::signature::identity::Identity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    str::FromStr,
    sync::Arc,
};
use tokio::sync::RwLock;

pub fn start_server(auth_state: AuthState) -> Server<AddrIncoming, IntoMakeService<Router>> {
    let app = Router::new()
        .route("/github/oauth/token", post(exchange_gh_token))
        .route("/github/user", get(gh_userinfo))
        .route("/eth/oauth/token", post(exchange_eth_token))
        .route("/eth/user", get(eth_userinfo))
        .route("/eth/rpc", post(eth_rpc))
        .layer(Extension(auth_state));
    Server::try_bind(&SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 3001))
        .unwrap()
        .serve(app.into_make_service())
}

#[derive(Default)]
struct GhUsersState {
    users:   HashMap<u64, GhUser>,
    next_id: u64,
}

impl GhUsersState {
    fn register(&mut self, user: GhUser) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.users.insert(id, user);
        id
    }
}

#[derive(Default)]
struct EthUsersState {
    users:   HashMap<u64, EthUser>,
    next_id: u64,
}

impl EthUsersState {
    fn register(&mut self, user: EthUser) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.users.insert(id, user);
        id
    }

    fn find_user_by_address(&self, addr: Address) -> Option<&EthUser> {
        self.users
            .values()
            .find(|user| user.wallet.address() == addr)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct GhUser {
    pub name:       String,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub struct EthUser {
    pub wallet: LocalWallet,
    pub nonce:  usize,
}

impl EthUser {
    fn address(&self) -> Address {
        self.wallet.address()
    }
}

#[derive(Clone, Debug)]
pub enum AnyTestUser {
    Eth(EthUser),
    Gh(GhUser),
}

#[derive(Clone, Debug)]
pub struct TestUser {
    pub id:   u64,
    pub user: AnyTestUser,
}

impl TestUser {
    pub fn identity(&self) -> Identity {
        match &self.user {
            AnyTestUser::Eth(wallet) => {
                Identity::from_str(&format!("eth|0x{}", hex::encode(wallet.address().0))).unwrap()
            }
            AnyTestUser::Gh(user) => {
                Identity::from_str(&format!("git|{}|{}", self.id, user.name)).unwrap()
            }
        }
    }

    pub fn is_eth(&self) -> bool {
        matches!(&self.user, AnyTestUser::Eth(_))
    }
}

#[derive(Clone, Default)]
pub struct AuthState {
    github_users: Arc<RwLock<GhUsersState>>,
    eth_users:    Arc<RwLock<EthUsersState>>,
}

impl AuthState {
    pub async fn register_gh_user(&self, user: GhUser) -> TestUser {
        let id = self.github_users.write().await.register(user.clone());
        TestUser {
            id,
            user: AnyTestUser::Gh(user),
        }
    }

    pub async fn register_eth_user(&self, user: EthUser) -> TestUser {
        let id = self.eth_users.write().await.register(user.clone());
        TestUser {
            id,
            user: AnyTestUser::Eth(user),
        }
    }

    pub async fn get_gh_user(&self, auth_code: u64) -> Option<GhUser> {
        self.github_users
            .read()
            .await
            .users
            .get(&auth_code)
            .map(Clone::clone)
    }

    pub async fn get_eth_user(&self, auth_code: u64) -> Option<EthUser> {
        self.eth_users
            .read()
            .await
            .users
            .get(&auth_code)
            .map(Clone::clone)
    }
}

#[derive(Debug, Deserialize)]
struct ExchangeRequest {
    code: u64,
}

async fn exchange_gh_token(
    Form(req): Form<ExchangeRequest>,
    Extension(state): Extension<AuthState>,
) -> (StatusCode, Json<Value>) {
    let user = state.get_gh_user(req.code).await;
    match user {
        Some(_) => (
            StatusCode::OK,
            Json(json!({
                "access_token": format!("token_of::{}", req.code),
                "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
                "token_type": "Bearer",
                "expires_in": 60
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Invalid code"})),
        ),
    }
}

async fn exchange_eth_token(
    Form(req): Form<ExchangeRequest>,
    Extension(state): Extension<AuthState>,
) -> (StatusCode, Json<Value>) {
    let user = state.get_eth_user(req.code).await;
    match user {
        Some(_) => (
            StatusCode::OK,
            Json(json!({
                "access_token": format!("token_of::{}", req.code),
                "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
                "token_type": "Bearer",
                "expires_in": 60
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Invalid code"})),
        ),
    }
}

async fn gh_userinfo(
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<AuthState>,
) -> (StatusCode, Json<Value>) {
    let token = auth.0.token();
    let code_str = *token
        .split("::")
        .collect::<Vec<_>>()
        .get(1)
        .expect("invalid auth token");
    let code = u64::from_str(code_str).expect("invalid auth token");
    let user = state.get_gh_user(code).await;
    match user {
        Some(user) => (
            StatusCode::OK,
            Json(json!({"login": user.name, "created_at": user.created_at, "id": code})),
        ),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid auth token"})),
        ),
    }
}

async fn eth_userinfo(
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<AuthState>,
) -> (StatusCode, Json<Value>) {
    let token = auth.0.token();
    let code_str = *token
        .split("::")
        .collect::<Vec<_>>()
        .get(1)
        .expect("invalid auth token");
    let code = u64::from_str(code_str).expect("invalid auth token");
    let user = state.get_eth_user(code).await;
    match user {
        Some(user) => (
            StatusCode::OK,
            Json(json!({
                "sub": format!("eip155:1:0x{}", hex::encode(user.address().0))
            })),
        ),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid auth token"})),
        ),
    }
}

async fn eth_rpc(
    Json(body): Json<serde_json::Value>,
    Extension(state): Extension<AuthState>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(body["method"].as_str().unwrap(), "eth_getTransactionCount");
    let addr = body
        .get("params")
        .unwrap()
        .get(0)
        .unwrap()
        .as_str()
        .unwrap();
    let state = state.eth_users.read().await;
    let user = state
        .find_user_by_address(Address::from_str(addr).unwrap())
        .unwrap();
    (
        StatusCode::OK,
        Json(json!({ "result": format!("0x{:x}", user.nonce) })),
    )
}

use crate::util::Secret;
use clap::Parser;
use oauth2::{basic::BasicClient, AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
use std::{num::ParseIntError, ops::Deref};

#[derive(Clone, Debug, PartialEq, Eq, Parser)]
pub struct EthAuthOptions {
    /// The block height where the users nonce is fetched from.
    #[clap(long, env, value_parser = dec_to_hex, default_value = "15565180")]
    pub nonce_verification_block: String,

    /// The minimum nonce required at the specified block height in order to
    /// participate.
    #[clap(long, env, default_value = "4")]
    pub min_nonce: u64,

    /// The Ethereum JSON-RPC endpoint to use.
    /// Defaults to the `AllThatNode` public node for testing.
    #[clap(
        long,
        env,
        default_value = "https://ethereum-mainnet-rpc.allthatnode.com"
    )]
    pub rpc_url: Secret,

    /// Sign-in-with-Ethereum `OAuth2` authorization url.
    #[clap(
        long,
        env,
        default_value = "https://oidc.signinwithethereum.org/authorize"
    )]
    pub auth_url: String,

    /// Sign-in-with-Ethereum `OAuth2` token url.
    #[clap(long, env, default_value = "https://oidc.signinwithethereum.org/token")]
    pub token_url: String,

    /// Sign-in-with-Ethereum `OAuth2` user info url.
    #[clap(
        long,
        env,
        default_value = "https://oidc.signinwithethereum.org/userinfo"
    )]
    pub userinfo_url: String,

    /// Sign-in-with-Ethereum `OAuth2` callback redirect url.
    #[clap(long, env, default_value = "http://127.0.0.1:3000/auth/callback/eth")]
    pub redirect_url: String,

    /// Sign-in-with-Ethereum `OAuth2` client access id.
    #[clap(long, env)]
    pub client_id: Secret,

    /// Sign-in-with-Ethereum `OAuth2` client access key.
    #[clap(long, env)]
    pub client_secret: Secret,
}

#[derive(Clone)]
pub struct EthOAuthClient {
    client: BasicClient,
}

impl Deref for EthOAuthClient {
    type Target = BasicClient;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

pub fn eth_oauth_client(options: &EthAuthOptions) -> EthOAuthClient {
    EthOAuthClient {
        client: BasicClient::new(
            ClientId::new(options.client_id.get_secret().to_owned()),
            Some(ClientSecret::new(
                options.client_secret.get_secret().to_owned(),
            )),
            AuthUrl::new(options.auth_url.clone()).unwrap(),
            Some(TokenUrl::new(options.token_url.clone()).unwrap()),
        )
        .set_redirect_uri(RedirectUrl::new(options.redirect_url.clone()).unwrap()),
    }
}

fn dec_to_hex(input: &str) -> Result<String, ParseIntError> {
    Ok(format!("0x{:x}", input.parse::<u64>()?))
}

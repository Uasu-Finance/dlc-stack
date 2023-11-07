use std::{
    future::{ready, Ready},
    rc::Rc,
    str::FromStr,
    sync::Mutex,
};

use actix_http::h1;
use actix_web::{
    dev::{self, Service, ServiceRequest, ServiceResponse, Transform},
    web::{self, Data},
    Error,
};
use futures_util::future::LocalBoxFuture;
use secp256k1::hashes::Hash;
use secp256k1::Message;
use secp256k1::{ecdsa::Signature, Secp256k1};
use secp256k1::{hashes::sha256, PublicKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ServerNonce, UnprotectedPaths};

pub struct Verifier;

#[derive(Deserialize, Debug, Serialize)]
pub struct AuthenticatedMessage {
    pub message: Value,
    pub nonce: String,
    pub public_key: String,
    pub signature: String,
}

impl<S: 'static, B> Transform<S, ServiceRequest> for Verifier
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = VerifySignatureMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(VerifySignatureMiddleware {
            service: Rc::new(service),
        }))
    }
}

pub struct VerifySignatureMiddleware<S> {
    // This is special: We need this to avoid lifetime issues.
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for VerifySignatureMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    dev::forward_ready!(service);

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let svc = self.service.clone();

        let unprotected_paths = req.app_data::<Data<UnprotectedPaths>>().unwrap();

        if unprotected_paths.paths.contains(&req.path().to_string()) {
            return Box::pin(async move {
                let res = svc.call(req).await?;
                Ok(res)
            });
        }

        let nonces = req.app_data::<Data<Mutex<ServerNonce>>>().unwrap();
        let nonces = nonces.lock().unwrap().nonces.clone();

        Box::pin(async move {
            let body = req.extract::<web::Bytes>().await.unwrap();

            let body_json = serde_json::from_slice::<AuthenticatedMessage>(&body).unwrap();

            let secp = Secp256k1::new();

            let sig: Signature = Signature::from_str(&body_json.signature).unwrap();
            let message =
                Message::from(sha256::Hash::hash(body_json.message.to_string().as_bytes()));
            let pub_key = PublicKey::from_str(&body_json.public_key).unwrap();

            req.set_payload(bytes_to_payload(body));
            let mut res = svc.call(req).await?;
            if secp.verify_ecdsa(&message, &sig, &pub_key).is_err()
                || !nonces.contains(&body_json.nonce)
                || body_json.nonce != body_json.message["nonce"]
            {
                *res.response_mut().status_mut() = actix_web::http::StatusCode::FORBIDDEN;
            }
            Ok(res)
        })
    }
}

fn bytes_to_payload(buf: web::Bytes) -> dev::Payload {
    let (_, mut pl) = h1::Payload::create(true);
    pl.unread_data(buf);
    dev::Payload::from(pl)
}

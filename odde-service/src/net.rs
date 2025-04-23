use http_body_util::{BodyExt, Full};
use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
};
use serde_json::Value;
use std::convert::Infallible;
use tokio::sync::mpsc::Sender;

const GIT_ACTIONS: &[&str] = &[
    "push", // Includes branch deletes
];

pub async fn git_wh(
    req: Request<Incoming>,
    request_git: Sender<()>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if let Ok(v) =
        serde_json::from_slice::<Value>(&req.collect().await.unwrap_or_default().to_bytes())
    {
        if v.get("action")
            .and_then(|v| Some(GIT_ACTIONS.contains(&v.to_string().as_str())))
            .unwrap_or(false)
        {
            let _ = request_git.send(()).await;
            return Ok(Response::new(Full::new(Bytes::from("yippee :)"))));
        }
    };

    Ok(Response::new(Full::new(Bytes::from(
        "ur not a webhook, shoo!",
    ))))
}

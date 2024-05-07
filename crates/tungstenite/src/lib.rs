use extractors::TCtxFunc;
use rspc::internal::jsonrpc;
use rspc::internal::jsonrpc::handle_json_rpc;
use rspc::internal::jsonrpc::Sender;
use rspc::internal::jsonrpc::SubscriptionMap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tungstenite::http::request::Parts;

mod extractors;

type WebSocket = tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>;

pub async fn handle_websocket<TCtx, TCtxFn, TCtxFnMarker>(
    ctx_fn: TCtxFn,
    mut socket: WebSocket,
    parts: Parts,
    router: Arc<rspc::Router<TCtx>>,
) where
    TCtx: Send + Sync + 'static,
    TCtxFn: TCtxFunc<TCtx, TCtxFnMarker>,
{
    use futures::SinkExt;
    use futures::StreamExt;
    use serde_json::Value;
    use tokio::sync::mpsc;
    use tungstenite::Message;

    let mut subscriptions = HashMap::new();
    let (mut tx, mut rx) = mpsc::channel::<jsonrpc::Response>(100);

    loop {
        tokio::select! {
            biased;
            msg = rx.recv() => {
                let msg = match serde_json::to_string(&msg) {
                    Ok(v) => v,
                    Err(_err) => {
                        // #[cfg(feature = "tracing")]
                        // tracing::error!("Error serializing websocket message: {}", _err);

                        continue;
                    }
                };

                match socket.send(Message::Text(msg)).await {
                    Ok(_) => {}
                    Err(_err) => {
                        // #[cfg(feature = "tracing")]
                        // tracing::error!("Error sending websocket message: {}", _err);

                        continue;
                    }
                }
            }
            msg = socket.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        let msg = match msg {
                            Message::Text(text) => serde_json::from_str::<Value>(&text),
                            Message::Binary(binary) => serde_json::from_slice(&binary),
                            Message::Ping(_) | Message::Pong(_) | Message::Close(_) | Message::Frame(_)=> {
                                continue;
                            }
                        };

                        match msg.and_then(|v| match v.is_array() {
                            true => serde_json::from_value::<Vec<jsonrpc::Request>>(v),
                            false => serde_json::from_value::<jsonrpc::Request>(v).map(|v| vec![v]),
                        }) {
                        Ok(reqs) => {
                            for request in reqs {
                                let ctx = ctx_fn.exec(parts.clone());

                                handle_json_rpc(match ctx {
                                    Ok(v) => v,
                                    Err(_err) => {
                                        #[cfg(feature = "tracing")]
                                        tracing::error!("Error executing context function: {}", _err);

                                        continue;
                                    }
                                }, request, &router, &mut Sender::Channel(&mut tx),
                                &mut SubscriptionMap::Ref(&mut subscriptions)).await;
                            }
                        },
                        Err(_err) => {
                            #[cfg(feature = "tracing")]
                            tracing::error!("Error parsing websocket message: {}", _err);

                            // TODO: Send report of error to frontend

                            continue;
                        }
                    };
                    }
                    Some(Err(_err)) => {
                        // #[cfg(feature = "tracing")]
                        // tracing::error!("Error receiving websocket message: {}", _err);

                        continue;
                    }
                    None => {
                        // #[cfg(feature = "tracing")]
                        // tracing::error!("Websocket closed");

                        return;
                    }
                }
            }
        }
    }
}

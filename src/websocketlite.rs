use std::time::Duration;
use std::{sync::Arc, error::Error};
use futures_util::stream::SplitSink;
use tokio::sync::{oneshot, Mutex};
use websocket_lite::Message;
use websocket_lite::AsyncNetworkStream;
use websocket_lite::MessageCodec;
use std::collections::HashMap;
use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use tokio_util::codec::Framed;
use crate::WsError;

struct LiteWriteReqmap {
    ws_write: SplitSink<Framed<Box<dyn AsyncNetworkStream + Send + Sync + Unpin>, MessageCodec>, Message>,
    reqmap: HashMap<u64, oneshot::Sender<String>>,
}

pub struct LiteWsRouter {
    write_reqmap: Arc<Mutex<LiteWriteReqmap>>,
}

impl LiteWsRouter {
    pub async fn new(node: &str, jwt: Option<String>) -> Result<LiteWsRouter, Box<dyn Error>> {
        let mut builder = websocket_lite::ClientBuilder::new(&node)?;
        if let Some(jwt) = jwt {
            builder.add_header("Authorization".to_string(), format!("Bearer {}", jwt));
        }

        let ws_stream = builder.async_connect().await.unwrap();

        let (ws_write, ws_read) = ws_stream.split();
        tracing::info!("Websocket connection to {} established.", node);

        let write_reqmap = Arc::new(Mutex::new(LiteWriteReqmap{
            ws_write,
            reqmap: HashMap::new(),
        }));

        let router = LiteWsRouter { write_reqmap: write_reqmap.clone() };

        let read_loop = ws_read.for_each_concurrent(None, move |msg| {
            let write_reqmap = write_reqmap.clone();
            async move {

                let msg = match msg {
                    Ok(msg) => msg,
                    Err(e) => {
                        tracing::error!("Error reading message: {}", e);
                        return;
                    }
                };

                let msg = match msg.as_text() {
                    Some(msg) => msg,
                    None => {
                        tracing::error!("Error reading message as text: {:?}", msg);
                        return;
                    }
                };

                if msg.is_empty() {
                    return;
                }

                let resp = match serde_json::from_str::<serde_json::Value>(&msg) {
                    Ok(resp) => resp,
                    Err(e) => {
                        tracing::error!("Error parsing message: {}", e);
                        return;
                    }
                };

                // Payload ID can come in as "1" or just 1
                let payload_id: u64 = match &resp["id"] {
                    serde_json::Value::Number(number) => {
                        if let Some(payload_id) = number.as_u64() {
                            payload_id
                        } else {
                            tracing::error!("Invalid payload ID format: {}", number);
                            return;
                        }
                    }
                    serde_json::Value::String(s) => match s.parse::<u64>() {
                        Ok(parsed_id) => parsed_id,
                        Err(e) => {
                            tracing::error!("Error parsing payload ID as u64: {}", e);
                            return;
                        }
                    },
                    _ => {
                        tracing::error!("Unexpected payload ID format: {:?}", resp["id"]);
                        return;
                    }
                };

                if let Some(tx) = write_reqmap.lock().await.reqmap.remove(&payload_id) {
                    let channelres = tx.send(msg.to_string());
                    if let Err(e) = channelres {
                        tracing::error!("Error sending message to channel: {}", e);
                    }
                } else {
                    tracing::warn!("No corresponding sender found for payload_id: {}", payload_id);
                }



            }
        });
        
        tokio::spawn(read_loop);

        Ok(router)
    }


    // make sure that the payload_id is the same id your using in your json rpc request
    pub async fn send(&self, req: String, payload_id: u64) -> Result<oneshot::Receiver<String>, WsError> {

        let (tx, rx) = oneshot::channel();
        let req = Message::text(req);
        let mut write_reqmap = self.write_reqmap.lock().await;
        write_reqmap.reqmap.insert(payload_id, tx);
        // send the req to the node
        let res = write_reqmap.ws_write.send(req).await;
        drop(write_reqmap); // drop here to yield the lock as soon as possible

        match res {
            Ok(()) => {},
            Err(e) => {
                tracing::error!("Problem sending message: {}", e);
                return Err(WsError::Other("Problem sending rpc request".into()));
            }
        }

        Ok(rx)
    }

    pub async fn make_request(&self, req: String, payload_id: u64) -> Result<String, WsError> {
        Ok(self.send(req, payload_id).await?.await.map_err(|e| WsError::Other(Box::new(e)))?)
    }

    pub async fn make_request_timeout(&self, req: String, payload_id: u64, timeout: Duration) -> Result<String, WsError> {
        let rx = self.send(req, payload_id).await?;
        tokio::time::timeout(timeout, rx).await.map_err(|_| WsError::Timeout)?.map_err(|e| WsError::Other(Box::new(e)))
    }

    pub async fn stop(&self) {
        tracing::debug!("Shutting down websocket connection.");
        // call close on the websocket connection
        self.write_reqmap.lock().await.ws_write.close().await.unwrap();
    }


}

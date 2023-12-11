use std::{collections::HashMap, error::Error, sync::Arc, time::Duration, str::FromStr};
use futures::{stream::SplitSink, SinkExt};
use futures_util::StreamExt;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::{Message, self}};
use tracing;
use tokio::{sync::oneshot, net::TcpStream, sync::Mutex};
use http::{Request, Uri};

pub mod websocketlite;


#[derive(Debug)]
pub enum WsError {
    Timeout,
    ConnectionClosed,
    AlreadyClosed,
    IoError(std::io::Error),
    Other(Box<dyn Error + Send + Sync>),
}

impl From<tungstenite::error::Error> for WsError {
    fn from(e: tungstenite::error::Error) -> Self {
        match e {
            tungstenite::error::Error::ConnectionClosed => WsError::ConnectionClosed,
            tungstenite::error::Error::AlreadyClosed => WsError::AlreadyClosed,
            tungstenite::error::Error::Io(e) => WsError::IoError(e),
            _ => WsError::Other(Box::new(e)),
        }
    }
}

struct WriteReqmap {
    ws_write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    reqmap: HashMap<u64, oneshot::Sender<String>>,
}

pub struct WsRouter {
    write_reqmap: Arc<Mutex<WriteReqmap>>,
}

impl WsRouter {
    pub async fn new(node: &str, jwt: Option<String>) -> Result<WsRouter, Box<dyn Error>> {
        let url = Uri::from_str(node)?;

        #[allow(deprecated)]
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
            write_buffer_size: 0,
            max_message_size: None,
            max_frame_size: None,
            max_write_buffer_size: usize::MAX,
            accept_unmasked_frames: false,
            max_send_queue: None,   // deprecated
        };

        let mut request = Request::builder()
            .method("GET")
            .uri(&url)
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .header("Host", url.host().unwrap());

        if let Some(jwt) = jwt {
            request = request.header("Authorization", format!("Bearer {}", jwt));
        }
        
        let request = request.body(())?;
            
        

        let (ws, _) = tokio_tungstenite::connect_async_with_config(request, Some(config), false).await?;
        let (ws_write, ws_read) = ws.split();
        tracing::info!("Websocket connection to {} established.", node);

        let write_reqmap = Arc::new(Mutex::new(WriteReqmap{
            ws_write,
            reqmap: HashMap::new(),
        }));
        let write_reqmap_clone = write_reqmap.clone(); // Clone for closure

        let router = WsRouter{
            write_reqmap: write_reqmap,
        };
        
        let read_loop = ws_read.for_each_concurrent(None, move |msg| {
            let write_reqmap = write_reqmap_clone.clone(); // Clone for closure
            
            async move {
                match msg {
                    Ok(msg) => {
                        let msg = msg.into_text().unwrap();

                        if msg.is_empty() {
                            return;
                        }

                        let resp: Result<serde_json::Value, serde_json::Error>  = serde_json::from_str(&msg);
                        let resp = match resp {
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
                            let channelres = tx.send(msg);
                            if let Err(e) = channelres {
                                tracing::error!("Error sending message to channel: {}", e);
                            }
                        } else {
                            tracing::warn!("No corresponding sender found for payload_id: {}", payload_id);
                        }
                    },
                    Err(e) => {
                        tracing::error!("Error reading message: {}", e);
                    }
                }
            }
        });

        tokio::spawn(read_loop);

        Ok(router)
    }


    // make sure that the payload_id is the same id your using in your json rpc request
    pub async fn send(&self, req: String, payload_id: u64) -> Result<oneshot::Receiver<String>, WsError> {

        let (tx, rx) = oneshot::channel();
        let req = Message::Text(req);
        let mut write_reqmap = self.write_reqmap.lock().await;
        write_reqmap.reqmap.insert(payload_id, tx);
        // send the req to the node
        write_reqmap.ws_write.send(req).await?;
        drop(write_reqmap); // drop here to yield the lock as soon as possible
        Ok(rx)
    }

    // makes + waits for response
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

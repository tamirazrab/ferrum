use futures_util::stream::SplitSink;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

#[derive(Debug)]
pub struct User {
    pub id: String,
    pub authenticated: bool,
    pub ws_stream: SplitSink<WebSocketStream<TcpStream>, Message>,
}

impl User {
    pub fn new(id: String, ws_stream: SplitSink<WebSocketStream<TcpStream>, Message>) -> Self {
        Self {
            id,
            authenticated: false,
            ws_stream,
        }
    }
}

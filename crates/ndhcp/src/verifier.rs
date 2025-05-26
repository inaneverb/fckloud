use std::net::IpAddr;
use anyhow::Result;
use hyper::{Response, client::conn::http1, service::{Service, HttpService}};
use serde::{Deserialize};
use serde_json;
use reqwest;

/// Capability to obtain the current machine's public local IP address,
/// taking in account local IP address the socket of outgoing connection
/// must be bound to.
pub(crate) trait GetPublicLocalIp {
    async fn get_public_ip(&self, local_ip: IpAddr) -> Result<IpAddr>;
}

struct HttpManager {
    
}

impl GetPublicLocalIp for HttpManager {
    async fn get_public_ip(&self, local_ip: IpAddr) -> Result<IpAddr> {
        
    }
}

mod decoders {
    use super::*;

    struct HttpBin(Response<Vec<u8>>);
    
    impl GetPublicLocalIp for HttpBin {
        async fn get_public_ip(&self, _: IpAddr) -> Result<IpAddr> {
            
            #[derive(Deserialize)]
            struct ResponseTyped {
                origin: IpAddr
            }
            
            let (sender, conn) = http1::handshake(io);
        
            
            let resp: ResponseTyped = serde_json::from_reader(self.0.body());
            
            self.0.body()
        }
    }
}

impl GetPublicLocalIp for Response<Vec<u8>> {
    async fn get_public_ip(&self, _: IpAddr) -> Result<IpAddr> {
        self.
    }
}



/// 
/// 
pub struct Http {
    
}

fn foo() {
    
}
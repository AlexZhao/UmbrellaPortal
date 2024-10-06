/**
 * Apache License 2.0
 *   Copyright Zhao Zhe(Alex)
 *  
 *  Home Used HTTP Proxy to Socks5 Proxy tunnel 
 *
 */
use socks5_impl::{client, Result};
use tokio::net::{TcpListener, TcpStream, lookup_host};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufStream};

use clap::Parser;

use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    config: String
}

#[derive(Deserialize, Debug)]
struct UpstreamConfig {
    socks5: String,
}

#[derive(Deserialize, Debug)]
struct Config {
    http_portal: String,
    upstreams: UpstreamConfig,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let config_file = File::open(args.config.as_str())?;
    let reader = BufReader::new(config_file);
    let config: Config = serde_json::from_reader(reader)?;

    let portal_config = Arc::new(config);

    let listener = TcpListener::bind(&portal_config.http_portal.as_str()).await?;

    loop {
        let (mut socket, _) = listener.accept().await?;
        let config = portal_config.clone();

        tokio::spawn(async move {
                let mut buf = [0; 1024];
                let mut connect_phase = true;

                let mut hstream = BufStream::new(socket);
                let mut s5stream = None;

                if connect_phase {
                        match hstream.read(&mut buf).await {
                                Ok(n) => {
                                        if n > 0 {
                                                match process_http_connect(&buf).await {
                                                        Ok((host, port)) => {
                                                                let res = process_socks5_connect(&host, port, &config).await;

                                                                match res {
                                                                        Ok(mut stream) => {
                                                                                let len = form_http_response(&mut buf);
                                                                                if let Ok(n) = hstream.write(&buf[..len]).await {
                                                                                        hstream.flush().await;

                                                                                        s5stream = Some(stream);
                                                                                        connect_phase = false;                                           
        
                                                                                } else {
                                                                                        println!("Not able to connect the socks5 and send response");
                                                                                        hstream.shutdown();
                                                                                        stream.shutdown();
                                                                                        return ;
                                                                                }
                                                                        },
                                                                        Err(e) => {
                                                                                println!("{:?}", e);
                                                                                hstream.shutdown();
                                                                                return ;
                                                                        }
                                                                }
                                                        },
                                                        Err(e) => {
                                                                println!("Not able to process the proxy connection request {:?}", e);
                                                        }
                                                }

                                        }
                                },
                                Err(e) => {
                                        println!("Failed to read from socket {:?}", e);
                                        hstream.shutdown();
                                        return ;
                                }
                        };
 
                        if connect_phase == false {
                                let mut recv_buf = [0 as u8; 2048];
                                let mut send_buf = [0 as u8; 2048];

                                let mut s5_stream = s5stream.unwrap();                                         

                                loop {
                                        tokio::select! {
                                                hres = hstream.read(&mut recv_buf) => {
                                                        match hres {
                                                                Ok(n) => {
                                                                        if n > 0 {
                                                                                s5_stream.write(&recv_buf[..n]).await;
                                                                                s5_stream.flush().await;
                                                                        } else {
                                                                                hstream.shutdown().await;
                                                                                s5_stream.shutdown().await;
                                                                                return;
                                                                        }
                                                                },
                                                                Err(e) => {
                                                                        s5_stream.shutdown();
                                                                        hstream.shutdown();
                                                                        return 
                                                                }
                                                        }
                                                }
                                                sres = s5_stream.read(&mut send_buf) => {
                                                        match sres {
                                                                Ok(n) => {
                                                                        if n > 0 {
                                                                                hstream.write(&send_buf[..n]).await;
                                                                                hstream.flush().await;
                                                                        } else {
                                                                                s5_stream.shutdown().await;
                                                                                hstream.shutdown().await;
                                                                                return
                                                                        }
                                                                },
                                                                Err(e) => {
                                                                        s5_stream.shutdown();
                                                                        hstream.shutdown();
                                                                        return
                                                                }
                                                        }
                                                }
                                        };
                                }
                        }
                }
        });
    }
}

/*
 * accept and parse http request with connect 
 */
async fn process_http_connect(buf: &[u8]) -> Result<(String, u16)> {
        let mut headers = [httparse::EMPTY_HEADER; 16];
        let mut req = httparse::Request::new(&mut headers);
        let _res = req.parse(buf).unwrap();
        if req.method.unwrap().to_uppercase() == "CONNECT" && ( req.version.unwrap() == 1 || req.version.unwrap() == 0) {
                let full_path = req.path.unwrap();
                let mut host_addr = full_path.to_string();

                for addr in lookup_host(full_path).await? {
                        if addr.is_ipv4() {
                                host_addr = addr.to_string();
                                break;
                        }
                }
                let slices = host_addr.as_str().split(':').collect::<Vec<&str>>();
                let host = slices[0].to_string();
                let port: u16 = slices[1].parse().unwrap();
                return Ok((host, port))
        } else {
                return Err(socks5_impl::Error::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe)))
        }

}

/**
 * send http response to browser 
 */
fn form_http_response(buf: &mut [u8]) -> usize {
        let ok_res="HTTP/1.1 200 Connection established\r\nProxy-Connection: Keep-Alive\r\n\r\n";
        buf[..ok_res.len()].copy_from_slice(ok_res.as_bytes());

        return ok_res.len()
}

/*
 * connect to socks5 server
 */
async fn process_socks5_connect(target: &str, port: u16, config: &Arc<Config>) -> Result<BufStream<TcpStream>> {
        let s5_sock = TcpStream::connect(config.upstreams.socks5.as_str()).await?;
        let mut s5_stream = BufStream::new(s5_sock);
        match client::connect(&mut s5_stream, (target, port), None).await {
                Ok(_res) => {
                        return Ok(s5_stream)
                },
                Err(e) => {
                        return Err(e)
                }
        }
}

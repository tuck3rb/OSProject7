use std::net::{TcpListener, TcpStream};
use std::thread;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::env;
use std::collections::HashMap;

struct Cache {
    map: HashMap<String, String>,
    request_counts: HashMap<String, u32>,
    capacity: usize,
}

impl Cache {
    fn new(capacity: usize) -> Cache {
        Cache {
            map: HashMap::new(),
            request_counts: HashMap::new(),
            capacity,
        }
    }

}

fn handle_client(mut stream: TcpStream, total_requests: Arc<Mutex<u32>>, valid_requests: Arc<Mutex<u32>>, stream_file: bool, cache: Option<Arc<Mutex<Cache>>>){  //option for cache 

    let peer_ip = stream.peer_addr().unwrap();
    println!("Received connection from: {}", peer_ip);

    let mut msg = String::new();

    let mut buffer = [0; 500];

    loop {
        // .read() with a 500 bite buffer
        let n = stream.read(&mut buffer[..]).unwrap();

        // Use std::str::from_utf8 to convert the buffer into a &str.
        let buffer = std::str::from_utf8(&buffer).unwrap();

        // Use the push_str() method to append it to the accumulated message.
        msg.push_str(buffer);

        // As the client is awaiting a reply, it will not close the connection. Even after it finishes its transmission, 
        // the read() will still block, waiting for more data. Since the http protocol specifies that a client's message ends 
        // with the character sequence \r\n\r\n, once the accumulated message ends with that sequence, the loop can end. 
        // As some clients end with \n\n, the http specification allows servers to end with that sequence too.
        if msg.contains("\r\n\r\n") || msg.contains("\n\n") {
            break;
        }
    }
    println!("Received message: {}", msg);

    let request_line = msg.lines().next().unwrap();

    let path = request_line.split_whitespace().nth(1).unwrap();
    // increment total request counter
    let mut total_requests = total_requests.lock().unwrap();
    *total_requests += 1;

    let mut response = String::new();
    let path_buf = PathBuf::from(path);

    // Validating
    let mut candidate_path = std::env::current_dir().unwrap();
    println!("cp1: {}", candidate_path.display());
    for i in path_buf.components().skip(1) {
        candidate_path.push(i);
        println!("i: {:?} cp: {}", i, candidate_path.display());
    }
    // call current dir again and make sure it is a parent
    let current_dir = std::env::current_dir().unwrap();

    // println!("cp: {}", candidate_path.display());
    // if subordanate
    println!("Here: {}", std::env::current_dir().unwrap().display());
    println!("cp2: {}", candidate_path.display());

    if candidate_path.starts_with(std::env::current_dir().unwrap()) {
            if !candidate_path.exists() {
                response = "HTTP/1.1 404 Not Found\r\n\r\n".to_string();
            } else if stream_file {
                let file = std::fs::File::open(&candidate_path).unwrap();
                let mut reader = std::io::BufReader::new(file);
                let mut buffer = [0u8; 1024];
                response = "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\n\r\n".to_string();
                loop {
                    let read_bytes = reader.read(&mut buffer).unwrap();
                    if read_bytes == 0 {
                        response.push_str("0\r\n\r\n");
                        break;
                    }
                    response.push_str(&format!("{:x}\r\n", read_bytes));
                    response.push_str(&String::from_utf8_lossy(&buffer[..read_bytes]));
                    response.push_str("\r\n");
                }
                let mut valid_requests = valid_requests.lock().unwrap();
                *valid_requests += 1;
            } else if let Some(cache) = &cache {
                // caching things

                let mut cache = cache.lock().unwrap();

                // 0. bump count of request in request counts
                let path_string = path.to_string();
                // let request_count = cache.request_counts.entry(path_string.clone()).or_insert(0);
                // *request_count += 1;
                let request_count = {
                    let request_count = cache.request_counts.entry(path_string.clone()).or_insert(0);
                    *request_count += 1;
                    *request_count
                };
                // 1. check to see if requested file is in map
                // 2. if yes, send whats in the map over the socket
                if cache.map.contains_key(&path_string) {
                    let contents = cache.map.get(&path_string).unwrap();
                    response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\n\r\n<html>\r\n<body>\r\n<h1>Message received</h1>\r\nRequested file: {}<br>\r\n</body>\r\n</html>\r\n",
                        contents.len(), contents
                    );
                }
                // 3. else, load it from disk and into a string (SEND IT)
                else {
                    let contents = std::fs::read_to_string(&candidate_path).unwrap();
                    response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\n\r\n<html>\r\n<body>\r\n<h1>Message received</h1>\r\nRequested file: {}<br>\r\n</body>\r\n</html>\r\n",
                        contents.len(), contents
                    );

                    // 4. if cache is below capacity add it to cache (map)
                    if cache.map.len() < cache.capacity {
                        cache.map.insert(path_string.clone(), contents.clone());
                    }                 
                    // 5. compare counts of everyone in cache and compare it to the one that just came in
                    // Source: https://dhghomon.github.io/easy_rust/Chapter_35.html
                    else {
                        let (min_key, min_count) = cache
                            .request_counts
                            .iter()
                            .min_by_key(|(_, &count)| count)
                            .map(|(key, &count)| (key.clone(), count))
                            .unwrap();
        
                        // 6. kick out the one with minimum count, add new request to cache
                        if request_count > min_count {
                            cache.map.remove(&min_key);
                            cache.request_counts.remove(&min_key);
                            cache.map.insert(path_string.clone(), contents.clone());
                        }
                    }
                }

                let mut valid_requests = valid_requests.lock().unwrap();
                *valid_requests += 1;

            } else {
                let contents = std::fs::read_to_string(&candidate_path).unwrap();
                response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\n\r\n<html>\r\n<body>\r\n<h1>Message received</h1>\r\nRequested file: {}<br>\r\n</body>\r\n</html>\r\n",
                    contents.len(), contents
                );
                let mut valid_requests = valid_requests.lock().unwrap();
                *valid_requests += 1;
            }
        //}
     } else {
            response = "HTTP/1.1 403 Forbidden\r\n\r\n".to_string();
    }


    println!("Response: {}", response);

    // Writing to browser
    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();

    // print both counts
    println!("Valid requests: {}", *valid_requests.lock().unwrap());
    println!("Total requests: {}", *total_requests);

}

fn main() -> std::io::Result<()> {

    let total_requests = Arc::new(Mutex::new(0));
    let valid_requests = Arc::new(Mutex::new(0));
    

    let listener = TcpListener::bind("127.0.0.1:8888")?;

    let args: Vec<String> = env::args().collect();
    let stream_file = args.contains(&String::from("-s"));

    let mut cache = None;

    for arg in &args {
        if arg.starts_with("-c=") {
            let capacity_str = &arg[3..]; // skip the "-c=" part
            // println!("{}", capacity_str);
            match capacity_str.parse::<usize>() {
                Ok(capacity) => {
                    cache = Some(Arc::new(Mutex::new(Cache::new(capacity))));
                }
                Err(e) => {
                    eprintln!("Error: Failed to parse cache capacity: {}", e);
                    std::process::exit(1);
                }
            }
            break;
        }
    }

    // accept connections and process them serially
    for stream in listener.incoming() {
        let stream = stream?;

        thread::spawn({
            let total_requests = total_requests.clone();
            let valid_requests = valid_requests.clone();
            let cache = cache.clone();

            move || {
                handle_client(stream, total_requests, valid_requests, stream_file, cache);
            }
        });
    }

    Ok(())
}

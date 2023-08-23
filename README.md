# ajsonrpc


Sssumes that the resp from the node is  
- valid json  
- has a id field  
- the id field is encoded as such: "1" (string)  


You shouldn't have a problem with this as long as your doing things correctly

You also should not drop the oneshot Reciever if using the send() function.

example benchmark comparing `ajsonrpc` with the `web3` library.  
Ran on a Ryzen 5 26000x and `geth` was on a Ryzen 9 5950x. All requests were `eth_syncing` requests.  
1 million requests were made concurrently.

| ajsonrpc                |  web3                |
|-------------------------|:--------------------:|
| 36.1s                   |  72.7s               |

`ajsonrpc` is 2.1x faster than `web3` in this case.

## Usage
Add this to your `Cargo.toml`:
```toml
[dependencies]
ajsonrpc = "0.1.0"
```

## Example
```rust
use ajsonrpc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    
    let router = ajsonrpc::WsRouter::new("ws://192.168.86.109:8546".to_string()).await?;

    let id = 1;

    let resp = router.make_request(format!(r#"{{"jsonrpc":"2.0","method":"eth_syncing","params":[],"id":"{}"}}"#, id), id).await?;

    println!("Response: {:?}", resp);

    Ok(())
}
```
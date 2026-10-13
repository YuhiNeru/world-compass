use crate::setup::Asset;
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
pub fn fetch(
    a: &Asset,
    partial: &Path,
    offset: u64,
    cancel: &AtomicBool,
    emit: &impl Fn(&str, u64, u64),
) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async{
  let client=reqwest::Client::builder().connect_timeout(Duration::from_secs(20)).timeout(Duration::from_secs(900)).user_agent("WorldCompass/0.1.0 (asset setup)").build().map_err(|e|e.to_string())?;
  let sending=client.get(&a.url).header("Range",format!("bytes={offset}-")).send();tokio::pin!(sending);
  let mut response=loop{tokio::select!{r=&mut sending=>{break r.map_err(|e|format!("通信失敗（再開可能）: {e}"))?;},_=tokio::time::sleep(Duration::from_millis(100))=>{if cancel.load(Ordering::SeqCst){return Err("中断しました。部分ダウンロードから再開できます。".into());}}}};
  if !response.status().is_success(){return Err(format!("取得失敗: HTTP {}",response.status()));}
  let mut done=offset;if response.status().as_u16()==206{let range=response.headers().get("content-range").and_then(|v|v.to_str().ok()).unwrap_or("");if !range.starts_with(&format!("bytes {offset}-"))||!range.ends_with(&format!("/{}",a.size)){return Err("不正な再開レスポンス".into());}}else{done=0;}
  let mut file=OpenOptions::new().create(true).write(true).append(done>0).truncate(done==0).open(partial).map_err(|e|e.to_string())?;
  loop{
   if cancel.load(Ordering::SeqCst){file.sync_all().map_err(|e|e.to_string())?;return Err("中断しました。再開ボタンで続けられます。".into());}
   let reading=response.chunk();tokio::pin!(reading);
   let chunk=loop{tokio::select!{r=&mut reading=>{break r.map_err(|e|e.to_string())?;},_=tokio::time::sleep(Duration::from_millis(100))=>{if cancel.load(Ordering::SeqCst){file.sync_all().map_err(|e|e.to_string())?;return Err("中断しました。再開ボタンで続けられます。".into());}}}};
   let Some(chunk)=chunk else{break;};done+=chunk.len() as u64;if done>a.size{return Err("配布サイズ上限を超えました".into());}file.write_all(&chunk).map_err(|e|e.to_string())?;emit(&a.id,done,a.size);
  }
  file.sync_all().map_err(|e|e.to_string())?;Ok(())
 })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
    };
    #[test]
    fn interrupted_download_resumes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut b = [0; 2048];
            stream.read(&mut b).unwrap();
            stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\nContent-Range: bytes 3-8/9\r\n\r\ndef").unwrap();
            signal.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
        });
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("part");
        std::fs::write(&path, b"abc").unwrap();
        let mut a = Asset {
            id: "test".into(),
            version: "1".into(),
            url: format!("http://{addr}"),
            size: 9,
            sha256: String::new(),
            license: "MIT".into(),
            license_url: String::new(),
            kind: "model".into(),
        };
        let start = std::time::Instant::now();
        assert!(fetch(&a, &path, 3, &cancel, &|_, _, _| {}).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        worker.join().unwrap();
        let offset = std::fs::metadata(&path).unwrap().len();
        assert!(offset >= 3);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        a.url = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut b = [0; 2048];
            let n = stream.read(&mut b).unwrap();
            assert!(String::from_utf8_lossy(&b[..n])
                .to_lowercase()
                .contains(&format!("range: bytes={offset}-")));
            let tail = &b"abcdefghi"[offset as usize..];
            write!(stream,"HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-8/9\r\n\r\n",tail.len(),offset).unwrap();
            stream.write_all(tail).unwrap();
        });
        cancel.store(false, Ordering::SeqCst);
        fetch(&a, &path, offset, &cancel, &|_, _, _| {}).unwrap();
        worker.join().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"abcdefghi");
    }
}

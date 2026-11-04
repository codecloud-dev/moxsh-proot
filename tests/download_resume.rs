//! 断点续传回归：本地最小 HTTP/1.1 服务（支持 Range / 206），不打外网。
//!
//! 复现用户实测事故：下载到一半被掐断，残留文件必须能通过 `curl -C -`
//! 续传补全，而不是被当成完整归档直接解压。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

fn start_range_server(body: Vec<u8>) -> (String, thread::JoinHandle<()>, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    // 克隆一份 listener 给服务线程（原 listener 保留在调用方用于控生命周期）
    let listener2 = listener.try_clone().unwrap();
    let handle = thread::spawn(move || {
        listener2.set_nonblocking(true).ok();
        let mut served = 0;
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while served < 4 && std::time::Instant::now() < deadline {
            match listener2.accept() {
                Ok((mut sock, _)) => {
                    served += 1;
                    sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let start: usize = req
                        .lines()
                        .find(|l| l.to_lowercase().starts_with("range:"))
                        .and_then(|l| l.split('=').nth(1))
                        .and_then(|v| v.trim().split('-').next())
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0)
                        .min(body.len());
                    let data = &body[start..];
                    let (code, extra) = if start > 0 {
                        (
                            "206 Partial Content",
                            format!(
                                "Content-Range: bytes {}-{}/{}\r\n",
                                start,
                                body.len().saturating_sub(1),
                                body.len()
                            ),
                        )
                    } else {
                        ("200 OK", String::new())
                    };
                    let head = format!(
                        "HTTP/1.1 {}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n{}\r\n",
                        code,
                        data.len(),
                        extra
                    );
                    let _ = sock.write_all(head.as_bytes());
                    let _ = sock.write_all(data);
                    let _ = sock.flush();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{}/rootfs.tar.gz", addr), handle, listener)
}

#[test]
fn curl_fetch_resumes_from_existing_part_file() {
    let body: Vec<u8> = (0..16384u32).map(|i| (i % 251) as u8).collect();
    let (url, handle, _listener) = start_range_server(body.clone());

    let dir = std::env::temp_dir().join(format!("mox-resume-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("rootfs.tar.gz.part");
    // 模拟「上次被超时掐断，只拿到前 4096 字节」
    std::fs::write(&part, &body[..4096]).unwrap();

    let mut progress = |_m: &str, _d: u64, _t: u64| {};
    let ok = mox_proot::proot::distro::curl_fetch(
        &url,
        &part,
        &mut progress,
        "测试源",
        body.len() as u64,
    )
    .expect("curl 应可执行");
    assert!(ok, "续传应成功");
    assert_eq!(std::fs::read(&part).unwrap(), body, "续传后内容必须逐字节一致");
    let _ = handle.join();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn curl_fetch_full_download_when_no_part_exists() {
    let body: Vec<u8> = b"mox-proot-full-download-test".repeat(64);
    let (url, handle, _listener) = start_range_server(body.clone());
    let dir = std::env::temp_dir().join(format!("mox-resume-full-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("f.part");
    let mut progress = |_m: &str, _d: u64, _t: u64| {};
    let ok = mox_proot::proot::distro::curl_fetch(&url, &part, &mut progress, "测试源", 0).unwrap();
    assert!(ok);
    assert_eq!(std::fs::read(&part).unwrap(), body);
    let _ = handle.join();
    let _ = std::fs::remove_dir_all(&dir);
}

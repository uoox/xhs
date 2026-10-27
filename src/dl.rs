use crate::error::{Result, XhsError};
use std::io::Read;
use std::path::{Path, PathBuf};

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

pub fn fetch(url: &str) -> Result<(Vec<u8>, String)> {
    let url = if url.starts_with("//") { format!("https:{url}") } else { url.to_string() };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(XhsError::arg(format!("不是图片地址: {url}")));
    }
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).build();
    let resp = agent
        .get(&url)
        .set("referer", "https://www.xiaohongshu.com/")
        .set("user-agent", UA)
        .set("accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
        .call()
        .map_err(|e| {
            XhsError::new(
                "download_error",
                format!("下载失败 {url}: {e}"),
                "图片地址带时效签名，过期就重新拉一次评论/笔记拿新地址",
            )
        })?;
    let ct = resp.header("content-type").unwrap_or("").to_string();
    let mut buf = vec![];
    resp.into_reader()
        .take(64 << 20)
        .read_to_end(&mut buf)
        .map_err(|e| XhsError::new("download_error", format!("读取图片失败: {e}"), "重试"))?;
    Ok((buf, ct))
}

pub fn ext_for(ct: &str, bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else if bytes.starts_with(b"\x89PNG") {
        "png"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.len() > 12 && &bytes[4..8] == b"ftyp" && (&bytes[8..12] == b"avif" || &bytes[8..12] == b"avis") {
        "avif"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.len() > 8 && &bytes[4..8] == b"ftyp" {
        "mp4"
    } else if ct.contains("png") {
        "png"
    } else if ct.contains("jpeg") {
        "jpg"
    } else if ct.contains("webp") {
        "webp"
    } else {
        "bin"
    }
}

pub fn save(url: &str, out: &Path, stem: &str) -> Result<PathBuf> {
    let (bytes, ct) = fetch(url)?;
    let target = if out.is_dir() || out.to_string_lossy().ends_with('/') {
        std::fs::create_dir_all(out).map_err(|e| XhsError::io(format!("建目录 {} 失败: {e}", out.display())))?;
        out.join(format!("{stem}.{}", ext_for(&ct, &bytes)))
    } else {
        if let Some(parent) = out.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| XhsError::io(format!("建目录 {} 失败: {e}", parent.display())))?;
            }
        }
        out.to_path_buf()
    };
    let tmp = target.with_extension("part");
    std::fs::write(&tmp, &bytes).map_err(|e| XhsError::io(format!("写 {} 失败: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &target).map_err(|e| XhsError::io(format!("写 {} 失败: {e}", target.display())))?;
    Ok(target)
}

pub fn url_stem(url: &str) -> String {
    let path = url.split(['?', '!', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("img");
    let st: String = last.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(60).collect();
    if st.is_empty() {
        "img".into()
    } else {
        st
    }
}

//! 영상 보관: 영상 파일을 보관 폴더 아래 로컬 디스크에 둔다. 서버에서만 쓴다.
//!
//! DB에는 보관 폴더 기준 상대 위치(영상 키, 예: `videos/12.mp4`)만 저장한다.
//! 나중에 원격 저장소로 바꿀 때 이 모듈만 바꾸면 되도록, 파일 다루는 일은 모두 여기에 모은다.

use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
pub struct VideoStore {
    root: PathBuf,
}

impl VideoStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 업로드를 받을 임시 파일을 만든다.
    ///
    /// 임시 파일은 값이 버려질 때(검사 실패, 연결 끊김, 오류) 저절로 지워진다.
    /// 최종 위치와 같은 디스크에 두어야 옮길 때 복사 없이 이름만 바뀐다.
    pub fn create_temp(&self) -> std::io::Result<NamedTempFile> {
        let dir = self.root.join("tmp");
        std::fs::create_dir_all(&dir)?;
        tempfile::Builder::new().prefix("upload-").suffix(".part").tempfile_in(dir)
    }

    /// 방송 번호로 영상 키를 짓는다.
    pub fn key_for(id: i64) -> String {
        format!("videos/{id}.mp4")
    }

    /// 다 받은 임시 파일을 영상 키 위치로 옮긴다.
    pub fn persist(&self, temp: NamedTempFile, key: &str) -> std::io::Result<()> {
        let path = self.path_of(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        temp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    }

    /// 옮긴 영상 파일을 지운다. 등록을 되돌릴 때 쓴다.
    pub fn remove(&self, key: &str) {
        let _ = std::fs::remove_file(self.path_of(key));
    }

    /// 영상 키가 가리키는 실제 파일 위치.
    pub fn path_of(&self, key: &str) -> PathBuf {
        self.root.join(Path::new(key))
    }
}

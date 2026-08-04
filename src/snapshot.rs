use std::fs;
use std::io::Write;
use std::path::PathBuf;

pub struct SnapshotSaver {
    dir: PathBuf,
}

impl SnapshotSaver {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn save(&self, data: &[u8]) -> std::io::Result<()> {
        let final_path = self.dir.join("latest_frame.h264");
        let tmp_path = self.dir.join(".latest_frame.h264.tmp");

        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(data)?;
        f.flush()?;
        fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }
}

impl Drop for SnapshotSaver {
    fn drop(&mut self) {
        if let Ok(exists) = fs::exists(&self.dir) {
            if exists {
                let _ = fs::remove_file(self.dir.join("latest_frame.h264"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_writes_and_overwrites() {
        let dir = std::env::temp_dir().join("mana_snapshot_test");
        let _ = fs::remove_dir_all(&dir);

        let saver = SnapshotSaver::new(dir.clone()).unwrap();

        saver.save(&[1, 2, 3]).unwrap();
        let data = fs::read(dir.join("latest_frame.h264")).unwrap();
        assert_eq!(data, vec![1, 2, 3]);

        saver.save(&[4, 5, 6, 7]).unwrap();
        let data = fs::read(dir.join("latest_frame.h264")).unwrap();
        assert_eq!(data, vec![4, 5, 6, 7]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_no_tmp_left_behind() {
        let dir = std::env::temp_dir().join("mana_snapshot_tmp_test");
        let _ = fs::remove_dir_all(&dir);

        let saver = SnapshotSaver::new(dir.clone()).unwrap();
        saver.save(b"hello").unwrap();

        assert!(!dir.join(".latest_frame.h264.tmp").exists());
        assert!(dir.join("latest_frame.h264").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}

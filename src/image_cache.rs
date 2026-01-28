use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use image::{DynamicImage, ImageReader};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{info, error};

use crate::error::Result;

#[derive(Clone)]
pub struct ImageCache {
    cache_dir: PathBuf,
    // In-memory cache of loaded images
    // We use Arc<Mutex> to allow sharing (though currently App is single threaded owner usually)
    memory_cache: Arc<Mutex<HashMap<String, DynamicImage>>>,
}

impl ImageCache {
    pub fn new() -> Result<Self> {
        let cache_dir = directories::ProjectDirs::from("com", "ernestoCruz05", "miru")
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Could not persist cache"))?
            .cache_dir()
            .join("images");

        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir)?;
        }

        Ok(Self {
            cache_dir,
            memory_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn get(&self, url: &str) -> Option<DynamicImage> {
        let key = self.hash_url(url);
        
        // 1. Check memory
        if let Ok(cache) = self.memory_cache.lock() {
            if let Some(img) = cache.get(&key) {
                return Some(img.clone());
            }
        }

        // 2. Check disk
        let path = self.cache_dir.join(&key);
        if path.exists() {
             let img_result = ImageReader::open(&path)
                .map_err(|e| e.to_string())
                .and_then(|r| r.with_guessed_format().map_err(|e| e.to_string()))
                .and_then(|r| r.decode().map_err(|e| e.to_string()));
            
            match img_result {
                Ok(img) => {
                    // Populate memory cache
                    if let Ok(mut cache) = self.memory_cache.lock() {
                        cache.insert(key, img.clone());
                    }
                    return Some(img);
                }
                Err(e) => {
                    error!("Failed to load cached image {}: {}", path.display(), e);
                }
            }
        }

        None
    }

    pub async fn download(&self, url: &str) -> Result<()> {
        let key = self.hash_url(url);
        let path = self.cache_dir.join(&key);

        if path.exists() {
            return Ok(());
        }

        info!("Downloading image: {}", url);
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;

        
        let mut file = fs::File::create(&path).await?;
        file.write_all(&bytes).await?;
        
        Ok(())
    }
    
    fn hash_url(&self, url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url);
        format!("{:x}", hasher.finalize())
    }
}

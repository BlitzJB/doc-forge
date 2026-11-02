//! Editor threads: the margin conversations about a document, kept per document in
//! the app's data folder (never next to the user's files). One JSON file per thread;
//! the frontend owns the shape, this side only stores it and lists it.

use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Threads {
    root: PathBuf,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMeta {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// Number of author messages.
    pub asks: usize,
}

/// Ids come from the frontend; only plain names, so nothing can point outside the folder.
fn safe(id: &str) -> Result<&str, String> {
    let ok = !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(id)
    } else {
        Err(format!("Invalid id {id:?}"))
    }
}

impl Threads {
    pub fn new(root: PathBuf) -> Self {
        Threads { root }
    }

    fn dir(&self, doc: &str) -> Result<PathBuf, String> {
        Ok(self.root.join(safe(doc)?))
    }

    fn file(&self, doc: &str, id: &str) -> Result<PathBuf, String> {
        Ok(self.dir(doc)?.join(format!("{}.json", safe(id)?)))
    }

    /// Newest first.
    pub fn list(&self, doc: &str) -> Result<Vec<ThreadMeta>, String> {
        let dir = self.dir(doc)?;
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(vec![]);
        };
        let mut out: Vec<ThreadMeta> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| read(&e.path()).ok())
            .filter_map(|v| meta(&v))
            .collect();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn load(&self, doc: &str, id: &str) -> Result<Value, String> {
        read(&self.file(doc, id)?)
    }

    pub fn save(&self, doc: &str, thread: &Value) -> Result<(), String> {
        let id = thread
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("A thread needs an id")?;
        let path = self.file(doc, id)?;
        fs::create_dir_all(self.dir(doc)?).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(thread).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::rename(tmp, path).map_err(|e| e.to_string())
    }

    pub fn delete(&self, doc: &str, id: &str) -> Result<(), String> {
        match fs::remove_file(self.file(doc, id)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

fn read(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn meta(v: &Value) -> Option<ThreadMeta> {
    let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    Some(ThreadMeta {
        id: v.get("id")?.as_str()?.to_string(),
        title: v
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        created_at: n("createdAt"),
        updated_at: n("updatedAt"),
        asks: v
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|m| {
                m.iter()
                    .filter(|x| x.get("role").and_then(|r| r.as_str()) == Some("user"))
                    .count()
            })
            .unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn saves_lists_newest_first_and_deletes() {
        let tmp = std::env::temp_dir().join(format!("forge-threads-{}", std::process::id()));
        let t = Threads::new(tmp.clone());
        let mk = |id: &str, at: u64| {
            json!({ "id": id, "title": id, "createdAt": at, "updatedAt": at,
            "messages": [{ "role": "user" }, { "role": "assistant" }, { "role": "user" }] })
        };
        t.save("doc1", &mk("a", 1)).unwrap();
        t.save("doc1", &mk("b", 2)).unwrap();
        let l = t.list("doc1").unwrap();
        assert_eq!(
            l.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(l[0].asks, 2);
        assert_eq!(t.load("doc1", "a").unwrap()["updatedAt"], 1);
        t.delete("doc1", "a").unwrap();
        t.delete("doc1", "a").unwrap();
        assert_eq!(t.list("doc1").unwrap().len(), 1);
        assert!(t.list("other").unwrap().is_empty());
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn refuses_ids_that_leave_the_folder() {
        let t = Threads::new(std::env::temp_dir());
        assert!(t.load("../x", "a").is_err());
        assert!(t.save("d", &json!({ "id": "../../evil" })).is_err());
        assert!(t.delete("d", "a/b").is_err());
    }
}

//! Product metadata and bound, precedence-aware library queries.
use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemDetails {
    pub description: String,
    pub shop: String,
    pub category: String,
    pub tags: Vec<String>,
    pub age_rating: String,
    pub is_starred: bool,
    pub release_notes: Option<String>,
}

pub(crate) fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let tx = conn.transaction()?;
    tx.execute_batch("ALTER TABLE packages ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}';")?;
    tx.pragma_update(None, "user_version", 6)?;
    tx.commit()?;
    Ok(())
}

impl Service {
    pub fn update_item(
        &self,
        item_id: &str,
        title: &str,
        mut details: ItemDetails,
    ) -> CoreResult<LibraryItem> {
        let title = booth_download::sanitize_booth_title(title)
            .ok_or(CoreError::Invalid("INVALID_TITLE"))?;
        if details.description.len() > 128 * 1024
            || details.shop.len() > 512
            || details.category.len() > 512
            || details.tags.len() > 100
            || details
                .tags
                .iter()
                .any(|tag| tag.is_empty() || tag.len() > 256)
            || details
                .release_notes
                .as_ref()
                .is_some_and(|value| value.len() > 32768)
            || !matches!(details.age_rating.as_str(), "" | "unknown" | "all" | "r18")
        {
            return Err(CoreError::Invalid("INVALID_METADATA"));
        }
        details.release_notes = release_notes(&details.description);
        details.tags.sort();
        details.tags.dedup();
        let conn = self.connect()?;
        if conn.execute(
            "UPDATE packages SET title=?2,metadata_json=?3 WHERE id=?1",
            params![item_id, title, serde_json::to_string(&details)?],
        )? != 1
        {
            return Err(CoreError::Invalid("ITEM_NOT_FOUND"));
        }
        self.load_item_by_id(item_id)
    }

    pub fn search_items(&self, query: &str) -> CoreResult<Vec<LibraryItem>> {
        let (predicate, values) = search_sql(query)?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT id,title FROM packages WHERE ({predicate}) ORDER BY title COLLATE NOCASE,id"
        ))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, title)| load_item(&conn, id, title))
            .collect()
    }
}

fn search_sql(query: &str) -> CoreResult<(String, Vec<String>)> {
    if query.len() > 4096 {
        return Err(CoreError::Invalid("INVALID_FILTER"));
    }
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = query.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '&' if !quoted && chars.peek() == Some(&'&') => {
                chars.next();
                if !word.is_empty() {
                    tokens.push(std::mem::take(&mut word));
                }
                tokens.push("&&".into());
            }
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    tokens.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if quoted {
        return Err(CoreError::Invalid("INVALID_FILTER"));
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    if tokens.is_empty() {
        return Ok(("1".into(), vec![]));
    }
    let mut groups = Vec::new();
    let mut group = Vec::new();
    let mut values = Vec::new();
    let mut and = false;
    for token in tokens {
        if token == "&&" {
            if group.is_empty() || and {
                return Err(CoreError::Invalid("INVALID_FILTER"));
            }
            and = true;
            continue;
        }
        if !group.is_empty() && !and {
            groups.push(format!("({})", group.join(" AND ")));
            group.clear();
        }
        and = false;
        let like = |value: &str| {
            format!(
                "%{}%",
                value
                    .to_lowercase()
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        };
        let clause = if let Some((field, value)) = token.split_once(':') {
            if value.is_empty() {
                return Err(CoreError::Invalid("INVALID_FILTER"));
            }
            match field {
                "shop" | "category" => {
                    values.push(like(value));
                    format!("lower(json_extract(metadata_json,'$.{field}')) LIKE ? ESCAPE '\\'")
                }
                "tag" => {
                    values.push(value.to_lowercase());
                    "EXISTS(SELECT 1 FROM json_each(metadata_json,'$.tags') WHERE lower(value)=?)"
                        .into()
                }
                "is" if value == "starred" => "json_extract(metadata_json,'$.isStarred')=1".into(),
                "age" if matches!(value, "r18" | "all") => {
                    values.push(value.into());
                    "json_extract(metadata_json,'$.ageRating')=?".into()
                }
                _ => return Err(CoreError::Invalid("INVALID_FILTER")),
            }
        } else {
            values.push(like(&token));
            values.push(like(&token));
            "(lower(title) LIKE ? ESCAPE '\\' OR EXISTS(SELECT 1 FROM package_versions WHERE package_id=packages.id AND lower(original_filename) LIKE ? ESCAPE '\\'))".into()
        };
        group.push(clause);
    }
    if and {
        return Err(CoreError::Invalid("INVALID_FILTER"));
    }
    groups.push(format!("({})", group.join(" AND ")));
    Ok((groups.join(" OR "), values))
}

pub(crate) fn release_notes(description: &str) -> Option<String> {
    let lines: Vec<_> = description.lines().collect();
    let start = lines.iter().position(|line| {
        matches!(
            line.trim()
                .trim_matches(['#', '【', '】', '[', ']', ' ', ':', '：'])
                .to_lowercase()
                .as_str(),
            "更新履歴"
                | "更新情報"
                | "changelog"
                | "change log"
                | "release notes"
                | "업데이트 내역"
        )
    })?;
    let body = lines[start + 1..]
        .iter()
        .take_while(|line| !line.trim().starts_with("## "))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let body: String = body.trim().chars().take(8192).collect();
    (!body.is_empty()).then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bound_queries_obey_and_precedence_and_validate_filters() {
        let service = Service::open(std::env::temp_dir().join(Uuid::new_v4().to_string())).unwrap();
        let conn = service.connect().unwrap();
        for (id, title, tags) in [
            ("a", "衣装", vec!["白"]),
            ("b", "髪", vec!["黒"]),
            ("c", "髪", vec!["白"]),
            ("d", "100% literal", vec![]),
        ] {
            conn.execute("INSERT INTO packages(id,title,identity_filename,created_at,metadata_json) VALUES(?1,?2,?1,?3,?4)",params![id,title,now(),serde_json::to_string(&ItemDetails{ tags:tags.into_iter().map(str::to_owned).collect(),..Default::default() }).unwrap()]).unwrap();
        }
        assert_eq!(service.search_items("衣装 髪 && tag:黒").unwrap().len(), 2);
        assert_eq!(service.search_items("\"100% literal\"").unwrap().len(), 1);
        assert_eq!(service.search_items("' OR 1=1 --").unwrap().len(), 0);
        for input in [
            "&& hair",
            "hair &&",
            "hair && && tag:黒",
            "unknown:x",
            "age:other",
            "\"unfinished",
        ] {
            assert!(service.search_items(input).is_err(), "{input}");
        }
        assert_eq!(release_notes("説明だけ"), None);
        assert_eq!(
            release_notes("説明\n【更新履歴】\n2026-11-18 修正\n## License"),
            Some("2026-11-18 修正".into())
        );
    }
}

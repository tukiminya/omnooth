//! Visibility authorization is process-local and never accepted from a client DTO.
use super::*;
use std::time::{Duration, Instant};

pub trait AuthenticationGateway {
    fn verify(&self, reason: &str) -> CoreResult<()>;
}
pub struct UnavailableAuthentication;
impl AuthenticationGateway for UnavailableAuthentication {
    fn verify(&self, _: &str) -> CoreResult<()> {
        Err(CoreError::Invalid("AUTH_UNAVAILABLE"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyState {
    pub available: bool,
    pub notice_acknowledged: bool,
    pub unlocked: bool,
    pub expires_at: Option<String>,
}

pub(crate) fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let tx = conn.transaction()?;
    tx.execute_batch("ALTER TABLE packages ADD COLUMN is_hidden INTEGER NOT NULL DEFAULT 0 CHECK(is_hidden IN (0,1));")?;
    tx.pragma_update(None, "user_version", 8)?;
    tx.commit()?;
    Ok(())
}
impl Service {
    pub fn privacy_state(&self) -> CoreResult<PrivacyState> {
        let acknowledged: bool = self.connect()?.query_row(
            "SELECT privacy_notice_acknowledged_at IS NOT NULL FROM app_settings WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let mut session = self
            .visibility
            .lock()
            .map_err(|_| CoreError::Invalid("AUTH_UNAVAILABLE"))?;
        if session
            .as_ref()
            .is_some_and(|(until, _)| Instant::now() >= *until)
        {
            *session = None;
        }
        Ok(PrivacyState {
            available: cfg!(any(target_os = "macos", target_os = "windows")),
            notice_acknowledged: acknowledged,
            unlocked: session.is_some(),
            expires_at: session.as_ref().map(|(_, expiry)| expiry.clone()),
        })
    }
    pub fn acknowledge_privacy_notice(&self) -> CoreResult<()> {
        self.connect()?.execute("UPDATE app_settings SET privacy_notice_acknowledged_at=COALESCE(privacy_notice_acknowledged_at,?1) WHERE id=1",[now()])?;
        Ok(())
    }
    pub fn lock_hidden_items(&self) -> CoreResult<()> {
        *self
            .visibility
            .lock()
            .map_err(|_| CoreError::Invalid("AUTH_UNAVAILABLE"))? = None;
        Ok(())
    }
    pub fn unlock_hidden_items(
        &self,
        auth: &dyn AuthenticationGateway,
    ) -> CoreResult<PrivacyState> {
        self.lock_hidden_items()?;
        if !self.privacy_state()?.notice_acknowledged {
            return Err(CoreError::Invalid("NOTICE_REQUIRED"));
        }
        let language = self.snapshot()?.settings.language;
        let reason = match language.as_str() {
            "en" => "Show hidden items in omnooth",
            "ko" => "omnooth에서 숨긴 상품 표시",
            _ => "omnooth の非表示アイテムを表示します",
        };
        auth.verify(reason)?;
        *self
            .visibility
            .lock()
            .map_err(|_| CoreError::Invalid("AUTH_UNAVAILABLE"))? = Some((
            Instant::now() + Duration::from_secs(300),
            (Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
        ));
        self.privacy_state()
    }
    pub fn set_item_hidden(&self, id: &str, hidden: bool) -> CoreResult<()> {
        let state = self.privacy_state()?;
        if !state.notice_acknowledged {
            return Err(CoreError::Invalid("NOTICE_REQUIRED"));
        }
        if !hidden && !state.unlocked {
            return Err(CoreError::Invalid("AUTH_REQUIRED"));
        }
        self.require_item_visible(id)?;
        // Hide children too so parent relationships do not reveal hidden product names.
        let conn = self.connect()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute("UPDATE packages SET is_hidden=?2 WHERE id IN (WITH RECURSIVE descendants(id) AS (SELECT id FROM packages WHERE id=?1 UNION SELECT p.id FROM packages p JOIN descendants d ON p.display_parent_id=d.id) SELECT id FROM descendants)",params![id,hidden])?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn require_item_visible(&self, id: &str) -> CoreResult<()> {
        let hidden: Option<bool> = self
            .connect()?
            .query_row("SELECT is_hidden FROM packages WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        match hidden {
            None => Err(CoreError::Invalid("ITEM_NOT_FOUND")),
            Some(true) if !self.privacy_state()?.unlocked => {
                Err(CoreError::Invalid("AUTH_REQUIRED"))
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn require_version_visible(&self, id: &str) -> CoreResult<()> {
        let package: String = self
            .connect()?
            .query_row(
                "SELECT package_id FROM package_versions WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(CoreError::Invalid("VERSION_NOT_FOUND"))?;
        self.require_item_visible(&package)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Success;
    impl AuthenticationGateway for Success {
        fn verify(&self, _: &str) -> CoreResult<()> {
            Ok(())
        }
    }
    #[test]
    fn visibility_cannot_be_bypassed_by_search_ids_or_restart() {
        let path = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let service = Service::open(&path).unwrap();
        let conn = service.connect().unwrap();
        conn.execute("INSERT INTO packages(id,title,identity_filename,created_at) VALUES('a','Hidden','a',?1)",[now()]).unwrap();
        assert_eq!(
            service.set_item_hidden("a", true).unwrap_err().code(),
            "NOTICE_REQUIRED"
        );
        assert_eq!(
            service.unlock_hidden_items(&Success).unwrap_err().code(),
            "NOTICE_REQUIRED"
        );
        service.acknowledge_privacy_notice().unwrap();
        service.set_item_hidden("a", true).unwrap();
        assert!(service.snapshot().unwrap().items.is_empty());
        assert!(service.search_items("Hidden anything").unwrap().is_empty());
        assert_eq!(
            service.load_item_by_id("a").unwrap_err().code(),
            "AUTH_REQUIRED"
        );
        assert!(service
            .unlock_hidden_items(&UnavailableAuthentication)
            .is_err());
        assert!(service.snapshot().unwrap().items.is_empty());
        service.unlock_hidden_items(&Success).unwrap();
        assert_eq!(service.snapshot().unwrap().items.len(), 1);
        assert!(Service::open(&path)
            .unwrap()
            .snapshot()
            .unwrap()
            .items
            .is_empty());
        *service.visibility.lock().unwrap() =
            Some((Instant::now() - Duration::from_secs(1), now()));
        assert!(service.snapshot().unwrap().items.is_empty());
    }
}

//! The shell of testing mobil. The page (the third entry of `ui/`) asks it over the bridge; it holds what the page must
//! not: the way to the hub, the device keys (Android Keystore, through appkit's plugin), the session (in memory), the
//! kept copy, the folder of a cloned vault. Every request of the page is a route of the app (`/api/testing/...`, the
//! push routes of the core); the shell adds the address, the origin, the session, the limits.
//!
//! The commands, and what they give the page:
//!
//! | command | what |
//! |---|---|
//! | `phone_state` | paired or not, the way, the name, whether this phone can confirm a trigger, the vault folder |
//! | `phone_scan` | the text of a QR code (the pairing link) |
//! | `phone_pair` | pair with a pairing link and a name |
//! | `phone_forget` | unpair: keys, copy, pairing, folder and push are removed (`MSEC-12`) |
//! | `link_fetch` | one request of the page to its hub |
//! | `link_trigger` | the request that starts something, confirmed on the phone (`MOB-03`) |
//! | `vault_pick`, `vault_list`, `vault_read`, `vault_off` | the folder of an export cloned with Git (read only) |
//! | `push_on`, `push_off`, `push_status` | notices of a red main (UnifiedPush) |

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use appkit_mobile::cache::{Cache, CacheLimits};
use appkit_mobile::link::{
    self, AppRoutes, BoxFuture, Limits, Link, LinkError, Method, NetError, SignError, Signer,
};
use appkit_mobile::state::{Paired, StateFile};
use appkit_mobile::way::{self, Way};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use tokio::sync::Mutex;

/// The app the link is for: its hub says `testing-desktop` in `hello`, its routes are below `/api/testing/`.
fn routes() -> AppRoutes {
    AppRoutes::new("testing-desktop", "testing")
}

/// The aliases of the two device keys in the Keystore.
const LOGIN_KEY: &str = "testing-mobile-login";
const TRIGGER_KEY: &str = "testing-mobile-trigger";
/// The largest file of an export that is read (`testing.export.MAX_DOC_BYTES`).
const MAX_EXPORT_FILE: u32 = 8 * 1024 * 1024;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A failure as the page reads it: one word (`appkit-ui/mobile`, `LinkFailure`).
#[derive(Debug, Serialize)]
pub struct Failure {
    kind: &'static str,
}

fn failure(kind: &'static str) -> Failure {
    Failure { kind }
}

impl From<LinkError> for Failure {
    fn from(e: LinkError) -> Self {
        failure(match e {
            LinkError::Net(NetError::Unreachable(_)) => "unreachable",
            LinkError::Net(NetError::AnswerLost) => "answer_lost",
            LinkError::Net(NetError::TooLarge) => "too_large",
            LinkError::Unpaired => "unpaired",
            LinkError::OtherHub => "other_hub",
            LinkError::NotAllowed => "not_allowed",
            LinkError::Sign(SignError::Cancelled) => "cancelled",
            LinkError::Sign(SignError::Gone) => "no_confirm",
            LinkError::OtherApp => "other_app",
            LinkError::Refused { .. } => "refused",
            _ => "failed",
        })
    }
}

// ── the keys ────────────────────────────────────────────────────────────────

/// A device key of the platform: the Android Keystore through appkit's plugin; on a desktop (the preview of a
/// developer) a key in memory that ends with the app.
struct DeviceKey<R: Runtime> {
    app: AppHandle<R>,
    alias: &'static str,
    /// Signs only after the biometrics or the device lock (the trigger key).
    user_auth: bool,
    #[cfg(not(target_os = "android"))]
    soft: Arc<link::SoftwareSigner>,
}

impl<R: Runtime> DeviceKey<R> {
    fn new(app: &AppHandle<R>, alias: &'static str, user_auth: bool) -> Self {
        Self {
            app: app.clone(),
            alias,
            user_auth,
            #[cfg(not(target_os = "android"))]
            soft: Arc::new(link::SoftwareSigner::random()),
        }
    }
}

#[cfg(target_os = "android")]
fn sign_error(word: &str) -> SignError {
    match word {
        "cancelled" => SignError::Cancelled,
        "gone" => SignError::Gone,
        "no_lock" => SignError::Failed("no_lock"),
        _ => SignError::Failed("failed"),
    }
}

impl<R: Runtime> Signer for DeviceKey<R> {
    fn public_key(&self) -> BoxFuture<'_, Result<String, SignError>> {
        #[cfg(not(target_os = "android"))]
        {
            self.soft.public_key()
        }
        #[cfg(target_os = "android")]
        {
            let app = self.app.clone();
            let (alias, auth) = (self.alias, self.user_auth);
            Box::pin(async move {
                tauri::async_runtime::spawn_blocking(move || {
                    let platform = app.state::<tauri_plugin_appkit::Platform<R>>();
                    platform
                        .public_key(alias, auth)
                        .map_err(|r| sign_error(&r.0))
                })
                .await
                .unwrap_or(Err(SignError::Failed("failed")))
            })
        }
    }

    fn sign<'a>(&'a self, message: &'a [u8]) -> BoxFuture<'a, Result<String, SignError>> {
        #[cfg(not(target_os = "android"))]
        {
            let _ = (&self.app, self.user_auth, self.alias);
            self.soft.sign(message)
        }
        #[cfg(target_os = "android")]
        {
            let app = self.app.clone();
            let (alias, auth) = (self.alias, self.user_auth);
            let message = message.to_vec();
            Box::pin(async move {
                tauri::async_runtime::spawn_blocking(move || {
                    let platform = app.state::<tauri_plugin_appkit::Platform<R>>();
                    platform
                        .sign(
                            alias,
                            &message,
                            auth,
                            "testing",
                            "Start the failed jobs again",
                        )
                        .map_err(|r| sign_error(&r.0))
                })
                .await
                .unwrap_or(Err(SignError::Failed("failed")))
            })
        }
    }
}

// ── the shell's state ───────────────────────────────────────────────────────

struct Shell {
    state_file: StateFile,
    cache: Cache,
    transport: Arc<dyn link::Transport>,
    link: Mutex<Option<Arc<Link>>>,
    /// The folder of a cloned export is chosen (the third way).
    vault: Mutex<bool>,
    vault_file: std::path::PathBuf,
    /// Written when the phone paired with a trigger key (it can confirm a trigger).
    confirm_file: std::path::PathBuf,
}

/// What the page knows of the pairing: never a key, a token or the address with its code.
#[derive(Serialize)]
pub struct PhoneState {
    paired: bool,
    way: Option<Way>,
    /// The name of the PC in the tailnet, or the port of the tunnel, to show (`MUI-05`).
    hub: Option<String>,
    name: Option<String>,
    /// This phone can confirm a trigger (it has a key behind its lock).
    confirm: bool,
    /// The folder of a cloned export is chosen.
    vault: bool,
}

fn state_of(paired: Option<&Paired>, vault: bool, confirm: bool) -> PhoneState {
    PhoneState {
        paired: paired.is_some(),
        way: if vault {
            Some(Way::Vault)
        } else {
            paired.map(|p| p.address.way())
        },
        hub: paired.map(|p| {
            p.address
                .base()
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_owned()
        }),
        name: paired.map(|p| p.name.clone()),
        confirm: paired.is_some() && confirm,
        vault,
    }
}

impl Shell {
    async fn current(&self) -> Option<Arc<Link>> {
        self.link.lock().await.clone()
    }
}

/// The link of the kept pairing, made once.
async fn link_of<R: Runtime>(app: &AppHandle<R>, shell: &Shell) -> Result<Arc<Link>, Failure> {
    let mut slot = shell.link.lock().await;
    if let Some(link) = slot.as_ref() {
        return Ok(Arc::clone(link));
    }
    let paired = shell
        .state_file
        .load()
        .map_err(|_| failure("failed"))?
        .ok_or_else(|| failure("unpaired"))?;
    let made = Arc::new(
        Link::new(
            Arc::clone(&shell.transport),
            Arc::new(DeviceKey::new(app, LOGIN_KEY, false)),
            routes(),
            paired,
            Limits::default(),
        )
        .with_confirm(Some(Arc::new(DeviceKey::new(app, TRIGGER_KEY, true)))),
    );
    *slot = Some(Arc::clone(&made));
    Ok(made)
}

#[tauri::command]
async fn phone_state(shell: State<'_, Shell>) -> Result<PhoneState, Failure> {
    let paired = shell.state_file.load().map_err(|_| failure("failed"))?;
    Ok(state_of(
        paired.as_ref(),
        *shell.vault.lock().await,
        shell.confirm_file.is_file(),
    ))
}

#[tauri::command]
async fn phone_scan<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, Failure> {
    #[cfg(target_os = "android")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<tauri_plugin_appkit::Platform<R>>()
                .scan_qr()
                .map_err(|_| failure("failed"))
        })
        .await
        .unwrap_or(Err(failure("failed")))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err(failure("unsupported"))
    }
}

#[derive(Deserialize)]
struct PairRequest {
    link: String,
    name: String,
}

#[tauri::command]
async fn phone_pair<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    request: PairRequest,
) -> Result<PhoneState, Failure> {
    let parsed = way::parse_pair_link(&request.link).ok_or_else(|| failure("bad_link"))?;
    let login: Arc<dyn Signer> = Arc::new(DeviceKey::new(&app, LOGIN_KEY, false));
    // The trigger key needs a screen lock; without one the phone pairs all the same and can only read.
    let trigger = DeviceKey::new(&app, TRIGGER_KEY, true);
    let confirm: Option<Arc<dyn Signer>> = match trigger.public_key().await {
        Ok(_) => Some(Arc::new(trigger)),
        Err(_) => None,
    };
    let can_confirm = confirm.is_some();
    let (paired, link) = link::pair(
        Arc::clone(&shell.transport),
        login,
        confirm,
        &routes(),
        &parsed,
        request.name.trim(),
        Limits::default(),
    )
    .await?;
    shell
        .state_file
        .save(&paired)
        .map_err(|_| failure("failed"))?;
    let _ = shell.cache.forget();
    *shell.link.lock().await = Some(Arc::new(link));
    *shell.vault.lock().await = false;
    let _ = std::fs::remove_file(&shell.vault_file);
    if can_confirm {
        std::fs::write(&shell.confirm_file, b"1").map_err(|_| failure("failed"))?;
    } else {
        let _ = std::fs::remove_file(&shell.confirm_file);
    }
    Ok(state_of(Some(&paired), false, can_confirm))
}

/// Remove everything of the pairing from the phone (`MSEC-12`): the link, the kept copy, the pairing, the folder, push,
/// and both keys. What could not be removed is named.
async fn wipe<R: Runtime>(app: &AppHandle<R>, shell: &Shell) -> Vec<&'static str> {
    *shell.link.lock().await = None;
    let mut left = Vec::new();
    if shell.cache.forget().is_err() {
        left.push("cache");
    }
    if shell.state_file.forget().is_err() {
        left.push("pairing");
    }
    *shell.vault.lock().await = false;
    let _ = std::fs::remove_file(&shell.vault_file);
    let _ = std::fs::remove_file(&shell.confirm_file);
    #[cfg(target_os = "android")]
    {
        let app = app.clone();
        let removed = tauri::async_runtime::spawn_blocking(move || {
            let platform = app.state::<tauri_plugin_appkit::Platform<R>>();
            let _ = platform.push_unregister();
            let _ = platform.forget_folder();
            platform.delete_key(LOGIN_KEY).is_ok() && platform.delete_key(TRIGGER_KEY).is_ok()
        })
        .await
        .unwrap_or(false);
        if !removed {
            left.push("keys");
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = app;
    left
}

/// The hub does not take this phone any more (revoked, or another hub answers): the phone forgets all of it and pairs
/// again (`MSEC-12`). The failure is passed on for the page to say.
async fn refused<R: Runtime>(app: &AppHandle<R>, shell: &Shell, e: LinkError) -> Failure {
    if matches!(e, LinkError::Unpaired | LinkError::OtherHub) {
        let _ = wipe(app, shell).await;
    }
    Failure::from(e)
}

#[tauri::command]
async fn phone_forget<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
) -> Result<PhoneState, Failure> {
    if wipe(&app, &shell).await.is_empty() {
        Ok(state_of(None, false, false))
    } else {
        Err(failure("failed"))
    }
}

/// One request of the page (`appkit-ui/mobile`, `BridgeRequest`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeRequest {
    method: String,
    path: String,
    body: Option<String>,
    if_none_match: Option<String>,
}

/// The answer (`BridgeResponse`); `kept_at` when it is the kept copy because the hub was not reached.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeResponse {
    status: u16,
    content_type: Option<String>,
    etag: Option<String>,
    body: String,
    kept_at: Option<u64>,
}

#[tauri::command]
async fn link_fetch<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    request: BridgeRequest,
) -> Result<BridgeResponse, Failure> {
    let link = link_of(&app, &shell).await?;
    match request.method.as_str() {
        "GET" => {
            let _ = request.if_none_match;
            let read = match link.read(&request.path, &shell.cache, now()).await {
                Ok(read) => read,
                Err(e) => return Err(refused(&app, &shell, e).await),
            };
            Ok(BridgeResponse {
                status: read.status,
                content_type: Some("application/json".to_owned()),
                etag: read.etag,
                body: read.body,
                kept_at: read.kept.then_some(read.fetched_at),
            })
        }
        "POST" => {
            let body = request.body.unwrap_or_else(|| "{}".to_owned());
            let r = match link
                .fetch(Method::Post, &request.path, body.as_bytes(), None)
                .await
            {
                Ok(r) => r,
                Err(e) => return Err(refused(&app, &shell, e).await),
            };
            Ok(BridgeResponse {
                status: r.status,
                content_type: r.content_type,
                etag: r.etag,
                body: String::from_utf8(r.body).map_err(|_| failure("failed"))?,
                kept_at: None,
            })
        }
        _ => Err(failure("not_allowed")),
    }
}

#[derive(Deserialize)]
struct TriggerRequest {
    path: String,
    body: String,
}

#[tauri::command]
async fn link_trigger<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    request: TriggerRequest,
) -> Result<BridgeResponse, Failure> {
    let link = link_of(&app, &shell).await?;
    let r = match link.trigger(&request.path, request.body.as_bytes()).await {
        Ok(r) => r,
        Err(e) => return Err(refused(&app, &shell, e).await),
    };
    Ok(BridgeResponse {
        status: r.status,
        content_type: r.content_type,
        etag: None,
        body: String::from_utf8(r.body).map_err(|_| failure("failed"))?,
        kept_at: None,
    })
}

// ── the cloned vault ────────────────────────────────────────────────────────

/// A path below the chosen folder: names joined by `/`, none empty, `.` or `..`, at most 8 deep.
fn plain_path(path: &str) -> bool {
    path.is_empty()
        || (path.len() <= 512
            && path.split('/').count() <= 8
            && path.split('/').all(|p| {
                !p.is_empty() && p != "." && p != ".." && !p.chars().any(char::is_control)
            }))
}

#[tauri::command]
async fn vault_pick<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
) -> Result<PhoneState, Failure> {
    #[cfg(target_os = "android")]
    {
        let app2 = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app2.state::<tauri_plugin_appkit::Platform<R>>()
                .pick_folder()
                .map_err(|r| {
                    if r.0 == "cancelled" {
                        failure("cancelled")
                    } else {
                        failure("failed")
                    }
                })
        })
        .await
        .unwrap_or(Err(failure("failed")))?;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (&app, &shell);
        Err(failure("unsupported"))
    }
    #[cfg(target_os = "android")]
    {
        std::fs::write(&shell.vault_file, b"{\"schema\":1}").map_err(|_| failure("failed"))?;
        *shell.vault.lock().await = true;
        let paired = shell.state_file.load().map_err(|_| failure("failed"))?;
        Ok(state_of(
            paired.as_ref(),
            true,
            shell.confirm_file.is_file(),
        ))
    }
}

#[tauri::command]
async fn vault_off(shell: State<'_, Shell>) -> Result<PhoneState, Failure> {
    *shell.vault.lock().await = false;
    let _ = std::fs::remove_file(&shell.vault_file);
    let paired = shell.state_file.load().map_err(|_| failure("failed"))?;
    Ok(state_of(
        paired.as_ref(),
        false,
        shell.confirm_file.is_file(),
    ))
}

#[tauri::command]
async fn vault_list<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> Result<Vec<tauri_plugin_appkit::Entry>, Failure> {
    if !plain_path(&path) {
        return Err(failure("not_allowed"));
    }
    #[cfg(target_os = "android")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<tauri_plugin_appkit::Platform<R>>()
                .list(&path)
                .map_err(|_| failure("failed"))
        })
        .await
        .unwrap_or(Err(failure("failed")))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err(failure("unsupported"))
    }
}

#[tauri::command]
async fn vault_read<R: Runtime>(app: AppHandle<R>, path: String) -> Result<String, Failure> {
    if !plain_path(&path) || path.is_empty() {
        return Err(failure("not_allowed"));
    }
    #[cfg(target_os = "android")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<tauri_plugin_appkit::Platform<R>>()
                .read(&path, MAX_EXPORT_FILE)
                .map_err(|r| {
                    if r.0 == "too_large" {
                        failure("too_large")
                    } else {
                        failure("failed")
                    }
                })
        })
        .await
        .unwrap_or(Err(failure("failed")))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, MAX_EXPORT_FILE);
        Err(failure("unsupported"))
    }
}

// ── push ────────────────────────────────────────────────────────────────────

/// What the page shows about notices.
#[derive(Serialize, Default)]
pub struct PushStatus {
    /// A UnifiedPush distributor is installed.
    distributor: bool,
    /// The person lets the app show notices.
    notices: bool,
    /// The hub has this phone's subscription for a red main.
    subscribed: bool,
}

#[tauri::command]
async fn push_status<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
) -> Result<PushStatus, Failure> {
    #[cfg(target_os = "android")]
    {
        let app2 = app.clone();
        let state = tauri::async_runtime::spawn_blocking(move || {
            app2.state::<tauri_plugin_appkit::Platform<R>>()
                .push_state()
        })
        .await
        .map_err(|_| failure("failed"))?
        .map_err(|_| failure("failed"))?;
        let _ = &shell;
        Ok(PushStatus {
            distributor: state.distributors > 0,
            notices: state.notices,
            subscribed: state.endpoint.is_some(),
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, shell);
        Ok(PushStatus::default())
    }
}

#[tauri::command]
async fn push_on<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
) -> Result<PushStatus, Failure> {
    let link = link_of(&app, &shell).await?;
    let r = link.fetch(Method::Get, "/api/push/key", &[], None).await?;
    let doc: serde_json::Value = serde_json::from_slice(&r.body).map_err(|_| failure("failed"))?;
    let vapid = doc["key"]
        .as_str()
        .ok_or_else(|| failure("refused"))?
        .to_owned();
    #[cfg(target_os = "android")]
    {
        let app2 = app.clone();
        let vapid2 = vapid.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app2.state::<tauri_plugin_appkit::Platform<R>>()
                .push_register(
                    &vapid2,
                    "testing",
                    "A main turned red. Open testing to see which.",
                )
        })
        .await
        .map_err(|_| failure("failed"))?
        .map_err(|r| {
            if r.0 == "no_distributor" {
                failure("no_distributor")
            } else {
                failure("failed")
            }
        })?;
        // The distributor answers with the endpoint on its own time: a few seconds.
        let mut endpoint = None;
        for _ in 0..40 {
            let app3 = app.clone();
            let state = tauri::async_runtime::spawn_blocking(move || {
                app3.state::<tauri_plugin_appkit::Platform<R>>()
                    .push_state()
            })
            .await
            .map_err(|_| failure("failed"))?
            .map_err(|_| failure("failed"))?;
            if let (Some(url), Some(key), Some(auth)) = (state.endpoint, state.p256dh, state.auth) {
                endpoint = Some((url, key, auth));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let (url, key, auth) = endpoint.ok_or_else(|| failure("no_endpoint"))?;
        let body = serde_json::json!({ "endpoint": url, "p256dh": key, "auth": auth, "kinds": ["ci_red"] }).to_string();
        let r = link
            .fetch(Method::Post, "/api/push/subscribe", body.as_bytes(), None)
            .await?;
        if r.status != 200 {
            return Err(failure("refused"));
        }
        return push_status(app, shell).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = vapid;
        Err(failure("unsupported"))
    }
}

#[tauri::command]
async fn push_off<R: Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
) -> Result<PushStatus, Failure> {
    if let Some(link) = shell.current().await {
        let _ = link
            .fetch(Method::Post, "/api/push/unsubscribe", b"{}", None)
            .await;
    }
    #[cfg(target_os = "android")]
    {
        let app2 = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            app2.state::<tauri_plugin_appkit::Platform<R>>()
                .push_unregister()
        })
        .await;
    }
    push_status(app, shell).await
}

/// The app.
///
/// # Panics
///
/// When Tauri cannot start (no WebView, a broken build).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_appkit::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let vault_file = dir.join("vault.json");
            let vault = vault_file.is_file();
            app.manage(Shell {
                state_file: StateFile::new(dir.join("pairing.json")),
                cache: Cache::open(dir.join("cache.json"), CacheLimits::default(), now()),
                transport: Arc::new(
                    appkit_mobile::net::NetTransport::new().map_err(std::io::Error::other)?,
                ),
                link: Mutex::new(None),
                vault: Mutex::new(vault),
                vault_file,
                confirm_file: dir.join("confirm.flag"),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            phone_state,
            phone_scan,
            phone_pair,
            phone_forget,
            link_fetch,
            link_trigger,
            vault_pick,
            vault_off,
            vault_list,
            vault_read,
            push_status,
            push_on,
            push_off,
        ])
        .run(tauri::generate_context!())
        .expect("testing mobil starts");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_of_the_vault_is_names_below_the_folder() {
        for ok in [
            "",
            "testing-export.json",
            "repos/lib.nvim-0123.json",
            "export/repos/a.json",
        ] {
            assert!(plain_path(ok), "{ok}");
        }
        for bad in [
            "/etc",
            "../x",
            "a//b",
            "a/./b",
            "a/../b",
            "a/b/c/d/e/f/g/h/i",
            "a\nb",
        ] {
            assert!(!plain_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_failure_of_the_link_is_one_word_of_the_page() {
        assert_eq!(Failure::from(LinkError::Unpaired).kind, "unpaired");
        assert_eq!(
            Failure::from(LinkError::Net(NetError::Unreachable("connect"))).kind,
            "unreachable"
        );
        assert_eq!(
            Failure::from(LinkError::Sign(SignError::Cancelled)).kind,
            "cancelled"
        );
        assert_eq!(Failure::from(LinkError::OtherHub).kind, "other_hub");
    }
}

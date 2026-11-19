//! Minimal compile-safe localization (ja / en). No external deps.
//! Every user-facing string key lives in `ALL`; both languages must define it
//! (enforced by the unit test below). Unknown keys render as the key itself.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

impl Lang {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "en" => Lang::En,
            _ => Lang::Ja,
        }
    }
}

/// All keys referenced by the UI. Kept in one place so tests can assert completeness.
#[allow(dead_code)] // used by unit tests
pub const ALL: &[&str] = &[
    "chrome.new",
    "chrome.settings",
    "chrome.fullscreen",
    "chrome.fullscreen_exit",
    "session.back",
    "session.refresh",
    "session.settings",
    "session.disconnect",
    "session.cancel",
    "session.close",
    "session.caps",
    "error.invalid_id",
    "error.name_host_required",
    "error.port",
    "error.host_chars",
    "error.duplicate_name",
    "error.duplicate_id",
    "error.unknown",
    "form.scale",
    "form.keymap",
    "form.keymap_linux",
    "form.keymap_windows",
    "form.keymap_identity",
    "home.saved",
    "home.connect",
    "home.edit",
    "home.sessions_open",
    "home.empty_title",
    "home.empty_body",
    "form.new_title",
    "form.edit_title",
    "form.name",
    "form.host",
    "form.port",
    "form.username",
    "form.password",
    "form.auto_reconnect",
    "form.fullscreen",
    "form.section_connect",
    "form.section_options",
    "form.section_security",
    "form.sec_local",
    "form.sec_legacy",
    "form.legacy_warn",
    "form.save",
    "form.delete",
    "form.delete_confirm",
    "settings.title",
    "settings.language",
    "settings.theme",
    "settings.theme_dark",
    "settings.theme_light",
    "settings.theme_system",
    "settings.scroll",
    "settings.scroll_reset",
    "settings.clipboard",
    "settings.clipboard_hint",
    "settings.display",
    "settings.remote",
    "settings.close",
    "settings.about",
    "settings.about_body",
    "viewer.scale_fit",
    "viewer.scale_one",
    "viewer.scale_stretch",
    "viewer.reconnect",
    "viewer.connecting",
    "status.saved",
    "status.save_failed",
    "status.not_found",
    "status.blocked",
    "status.keychain_unavailable",
    "status.connected",
    "status.connecting",
    "status.closed",
    "status.ready",
    "status.delete_done",
    "status.delete_keychain_failed",
];

pub fn t(lang: Lang, key: &str) -> &'static str {
    match lang {
        Lang::Ja => ja(key),
        Lang::En => en(key),
    }
}

fn ja(key: &str) -> &'static str {
    match key {
        "chrome.new" => "新規プロファイル",
        "chrome.settings" => "設定",
        "chrome.fullscreen" => "全画面",
        "chrome.fullscreen_exit" => "全画面を終了",
        "session.back" => "接続一覧",
        "session.refresh" => "再描画",
        "session.settings" => "設定",
        "session.disconnect" => "切断",
        "session.cancel" => "キャンセル",
        "session.close" => "閉じる",
        "session.caps" => "CAPS",
        "error.invalid_id" => "プロファイルIDが不正です",
        "error.name_host_required" => "名前とホスト名を入力してください",
        "error.port" => "ポートは1〜65535で指定してください",
        "error.host_chars" => "ホスト名に使用できない文字が含まれています",
        "error.duplicate_name" => "同じ名前のプロファイルが既にあります",
        "error.duplicate_id" => "同じIDのプロファイルが既にあります",
        "error.unknown" => "設定を検証できませんでした",
        "form.scale" => "表示スケール",
        "form.keymap" => "キーマップ",
        "form.keymap_linux" => "Linux",
        "form.keymap_windows" => "Windows",
        "form.keymap_identity" => "Itself (日本語/直接)",
        "home.saved" => "保存済み接続",
        "home.connect" => "接続",
        "home.edit" => "編集",
        "home.sessions_open" => "件接続中",
        "home.empty_title" => "まだ保存された接続はありません",
        "home.empty_body" => "右のフォームに接続先を入力して保存すると、ここに一覧で表示されます。",
        "form.new_title" => "新規接続",
        "form.edit_title" => "接続を編集",
        "form.name" => "名前",
        "form.host" => "ホスト名 / IP",
        "form.port" => "ポート",
        "form.username" => "ユーザー名",
        "form.password" => "パスワード",
        "form.auto_reconnect" => "ネットワーク障害後に自動で再接続",
        "form.fullscreen" => "接続後に全画面表示",
        "form.section_connect" => "接続情報",
        "form.section_options" => "オプション",
        "form.section_security" => "セキュリティ",
        "form.sec_local" => "ローカルホストのみ",
        "form.sec_legacy" => "レガシー (暗号化なし)",
        "form.legacy_warn" => "レガシーRFBは暗号化されていません。信頼できる経路またはSSH/WireGuardトンネル経由でのみ使用してください。",
        "form.save" => "保存する",
        "form.delete" => "削除する",
        "form.delete_confirm" => "もう一度押すと削除します",
        "settings.title" => "設定",
        "settings.language" => "言語",
        "settings.theme" => "テーマ",
        "settings.theme_dark" => "ダーク",
        "settings.theme_light" => "ライト",
        "settings.theme_system" => "システムに合わせる",
        "settings.scroll" => "スクロール速度",
        "settings.scroll_reset" => "リセット",
        "settings.clipboard" => "クリップボードをリモートと共有",
        "settings.clipboard_hint" => "テキストのみ (双方向)。パスワードなど機密情報が含まれる場合があります。",
        "settings.display" => "表示",
        "settings.remote" => "リモート接続",
        "settings.close" => "閉じる",
        "settings.about" => "このアプリについて",
        "settings.about_body" => "GlintVNC — Intel Mac 向けネイティブVNCビューア。ad-hoc署名配布のため、初回起動は右クリック→「開く」で起動してください。設定は ~/Library/Application Support/GlintVNC/config.json に保存され、パスワードはKeychainにのみ保存されます。",
        "viewer.scale_fit" => "全体表示 (Fit)",
        "viewer.scale_one" => "原寸 (1:1)",
        "viewer.scale_stretch" => "拡大 (Stretch)",
        "viewer.reconnect" => "再接続",
        "viewer.connecting" => "リモートデスクトップに接続しています…",
        "status.saved" => "保存しました",
        "status.save_failed" => "保存に失敗しました",
        "status.not_found" => "プロファイルが見つかりません",
        "status.blocked" => "暗号化されたVNC転送は未実装です。信頼できるネットワークではプロファイルのセキュリティを「レガシー」に明示的に変更してください。",
        "status.keychain_unavailable" => "Keychainにアクセスできません",
        "status.connected" => "接続しました",
        "status.connecting" => "接続しています…",
        "status.closed" => "切断されました",
        "status.ready" => "準備完了 — 左の一覧から接続を選ぶか、新しいプロファイルを作成してください",
        "status.delete_done" => "プロファイルを削除しました",
        "status.delete_keychain_failed" => "プロファイルは削除しましたが、Keychainの削除に失敗しました",
        _ => "?",
    }
}

fn en(key: &str) -> &'static str {
    match key {
        "chrome.tagline" => "VNC · macOS Intel",
        "chrome.new" => "New profile",
        "chrome.settings" => "Settings",
        "chrome.back" => "Connections",
        "chrome.fullscreen" => "Full screen",
        "chrome.fullscreen_exit" => "Exit full screen",
        "chrome.session_panel" => "Session",
        "session.back" => "Connections",
        "session.refresh" => "Refresh",
        "session.settings" => "Settings",
        "session.disconnect" => "Disconnect",
        "session.cancel" => "Cancel",
        "session.close" => "Close",
        "session.caps" => "CAPS",
        "error.invalid_id" => "Invalid profile ID",
        "error.name_host_required" => "Name and host are required",
        "error.port" => "Port must be between 1 and 65535",
        "error.host_chars" => "Host contains forbidden characters",
        "error.duplicate_name" => "A profile with this name already exists",
        "error.duplicate_id" => "A profile with this ID already exists",
        "error.unknown" => "Could not validate the settings",
        "form.scale" => "Display scale",
        "form.keymap" => "Key map",
        "form.keymap_linux" => "Linux",
        "form.keymap_windows" => "Windows",
        "form.keymap_identity" => "Identity (direct)",
        "home.saved" => "Saved connections",
        "home.empty" => "No profiles yet.\nCreate your first connection with the form on the right.",
        "home.connect" => "Connect",
        "home.edit" => "Edit",
        "home.count" => "items",
        "home.sessions_open" => "open",
        "home.empty_title" => "No saved connections yet",
        "home.empty_body" => "Fill in the form on the right and save to list your first connection here.",
        "form.new_title" => "New connection",
        "form.edit_title" => "Edit connection",
        "form.name" => "Name",
        "form.host" => "Hostname / IP",
        "form.port" => "Port",
        "form.username" => "Username",
        "form.password" => "Password",
        "form.auto_reconnect" => "Auto-reconnect after network failure",
        "form.fullscreen" => "Full screen after connect",
        "form.security" => "Security",
        "form.section_connect" => "Connection",
        "form.section_options" => "Options",
        "form.section_security" => "Security",
        "form.sec_local" => "Localhost only",
        "form.sec_legacy" => "Legacy (unencrypted)",
        "form.legacy_warn" => "Legacy RFB is unencrypted. Use only over trusted links or an SSH/WireGuard tunnel.",
        "form.save" => "Save profile",
        "form.delete" => "Delete",
        "form.delete_confirm" => "Press again to confirm delete",
        "form.name_required" => "Name and host are required",
        "settings.title" => "Settings",
        "settings.language" => "Language",
        "settings.theme" => "Theme",
        "settings.theme_dark" => "Dark",
        "settings.theme_light" => "Light",
        "settings.theme_system" => "Match system",
        "settings.scroll" => "Scroll speed",
        "settings.scroll_hint" => "1.00× = standard. Wheel sensitivity on the remote desktop.",
        "settings.scroll_reset" => "Reset",
        "settings.clipboard" => "Share clipboard with the remote",
        "settings.clipboard_hint" => "Text only (two-way). May contain sensitive data such as passwords.",
        "settings.display" => "Display",
        "settings.remote" => "Remote connection",
        "settings.close" => "Close",
        "settings.saved" => "Settings saved",
        "settings.about" => "About",
        "settings.about_body" => "GlintVNC — native VNC viewer for Intel Macs. Distributed ad-hoc signed; first launch via right-click → Open. Settings live in ~/Library/Application Support/GlintVNC/config.json; passwords are stored only in Keychain.",
        "viewer.display" => "Display",
        "viewer.scale_fit" => "Fit",
        "viewer.scale_one" => "1:1 (Pixel perfect)",
        "viewer.scale_stretch" => "Stretch",
        "viewer.scroll" => "Scroll speed",
        "viewer.session" => "Session",
        "viewer.reconnect" => "Reconnect",
        "viewer.disconnect" => "Disconnect",
        "viewer.connecting" => "Connecting to the remote desktop…",
        "viewer.session_persistence_note" => "Display mode is saved to this profile. Scroll speed is a global setting.",
        "status.saved" => "Saved",
        "status.save_failed" => "Save failed",
        "status.not_found" => "Profile not found",
        "status.blocked" => "Encrypted VNC transport is not implemented. For a trusted network, explicitly set security to Legacy in the profile.",
        "status.keychain_unavailable" => "Keychain unavailable",
        "status.connected" => "Connected",
        "status.connecting" => "Connecting…",
        "status.closed" => "Connection closed",
        "status.ready" => "Ready — pick a profile or create a new connection",
        "status.delete_done" => "Profile deleted",
        "status.delete_keychain_failed" => "Profile removed; Keychain cleanup failed",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_key_has_both_languages() {
        for key in ALL {
            let ja = ja(key);
            let en = en(key);
            assert_ne!(ja, *key, "missing ja for {key}");
            // chrome.tagline etc. are identical in both; only require non-empty.
            assert!(!en.is_empty(), "missing en for {key}");
            assert!(!ja.is_empty(), "missing ja for {key}");
        }
    }
    #[test]
    fn no_stale_or_unknown_ui_keys() {
        let src = include_str!("ui.rs");
        // These are selected at runtime by `configuration::validation_i18n_key`,
        // so they cannot appear as literals in ui.rs.
        let dynamic = [
            "error.invalid_id",
            "error.name_host_required",
            "error.port",
            "error.host_chars",
            "error.duplicate_name",
            "error.duplicate_id",
            "error.unknown",
        ];
        for key in ALL {
            assert!(
                dynamic.contains(key) || src.contains(&format!("\"{key}\"")),
                "unused i18n key (remove it or wire it up): {key}"
            );
        }
        fn literals<'a>(src: &'a str, marker: &str) -> Vec<&'a str> {
            let mut out = Vec::new();
            let mut rest = src;
            while let Some(i) = rest.find(marker) {
                rest = &rest[i + marker.len()..];
                if let Some(j) = rest.find('"') {
                    out.push(&rest[..j]);
                    rest = &rest[j + 1..];
                } else {
                    break;
                }
            }
            out
        }
        let mut used = Vec::new();
        for marker in [
            "self.tr(\"",
            "this.tr(\"",
            "i18n::t(lang, \"",
            "i18n::t(self.lang(), \"",
        ] {
            used.extend(literals(src, marker));
        }
        for key in used {
            assert!(ALL.contains(&key), "UI uses a key missing from ALL: {key}");
        }
    }
    #[test]
    fn lang_from_setting() {
        assert_eq!(Lang::from_setting("en"), Lang::En);
        assert_eq!(Lang::from_setting("ja"), Lang::Ja);
        assert_eq!(Lang::from_setting("fr"), Lang::Ja); // fail safe to ja default
        assert_eq!(Lang::from_setting(""), Lang::Ja);
    }
}

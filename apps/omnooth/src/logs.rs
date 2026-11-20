use gpui::{Context, Div, prelude::*, px, rgb};

use crate::{
    Omnooth,
    dropdown::DropdownId,
    ui::{
        BORDER, ButtonStyle::Outline, CONTENT_INSET, MONO_FONT, TEXT, button, column, divider,
        field, label, muted, page_title, row, scroll_area,
    },
};

const LOG_LEVEL: DropdownId = ("log-level", 0);
const LOG_LEVELS: &[&str] = &["すべてのレベル", "INFO", "WARN", "ERROR"];

impl Omnooth {
    pub fn logs(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        let filter = self.dropdowns.choice(LOG_LEVEL);
        let entries: Vec<_> = LOGS
            .iter()
            .copied()
            .filter(|&(_, level, _, _)| filter == 0 || level == LOG_LEVELS[filter])
            .collect();
        let count = |level| entries.iter().filter(|entry| entry.1 == level).count();
        let last_updated = entries.last().map_or("—", |entry| entry.0);
        let summary = format!(
            "INFO {} / WARN {} / ERROR {}　 最終更新 {last_updated}",
            count("INFO"),
            count("WARN"),
            count("ERROR")
        );

        column()
            .size_full()
            .px(CONTENT_INSET)
            .pt(px(24.))
            .pb(px(24.))
            .gap(px(18.))
            .child(column().gap(px(4.)).child(page_title("Logs")).child(muted(
                "取り込み・バックアップ・Unity への適用を、時系列で確認できます。",
                13.,
            )))
            .child(
                row()
                    .gap(px(12.))
                    .flex_shrink_0()
                    .child(field().child(muted("ログを検索…", 13.)))
                    .child(self.select(LOG_LEVEL, 200., LOG_LEVELS, cx))
                    .child(button(Outline, "ログをコピー"))
                    .child(button(Outline, "書き出す")),
            )
            .child(
                row()
                    .gap(px(12.))
                    .flex_shrink_0()
                    .child(label("2026-11-19", 14.))
                    .child(muted("現在のセッション", 13.).flex_1())
                    .child(self.checkbox(("follow-logs", 0), true, cx))
                    .child(label("最新のログを追従", 14.)),
            )
            .child(log_console(compact, filter, &entries))
            .child(
                row()
                    .justify_between()
                    .flex_shrink_0()
                    .child(muted(format!("{} 件のログ", entries.len()), 13.))
                    .child(muted(summary, 13.)),
            )
    }
}

type LogEntry = (&'static str, &'static str, &'static str, &'static str);

fn log_console(compact: bool, filter: usize, entries: &[LogEntry]) -> Div {
    let (time_width, process_width, gap) = if compact {
        (154., 144., 16.)
    } else {
        (184., 210., 24.)
    };

    column()
        .flex_1()
        .min_h_0()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(BORDER))
        .overflow_hidden()
        .child(
            row()
                .h(px(40.))
                .flex_shrink_0()
                .px(px(16.))
                .gap(px(gap))
                .child(muted("日時", 13.).w(px(time_width)).flex_shrink_0())
                .child(muted("レベル", 13.).w(px(64.)).flex_shrink_0())
                .child(muted("処理", 13.).w(px(process_width)).flex_shrink_0())
                .child(muted("メッセージ", 13.)),
        )
        .child(divider())
        .child(
            scroll_area(("log-rows", filter)).children(entries.iter().map(
                |&(time, level, process, message)| {
                    let color = match level {
                        "WARN" => 0xf2cd83,
                        "ERROR" => 0xffa7a7,
                        _ => TEXT,
                    };
                    row()
                        .min_h(px(36.))
                        .flex_shrink_0()
                        .px(px(16.))
                        .py(px(6.))
                        .gap(px(gap))
                        .child(
                            muted(format!("2026-11-19 {time}"), 13.)
                                .font_family(MONO_FONT)
                                .w(px(time_width))
                                .flex_shrink_0(),
                        )
                        .child(
                            label(level, 13.)
                                .font_family(MONO_FONT)
                                .text_color(rgb(color))
                                .w(px(64.))
                                .flex_shrink_0(),
                        )
                        .child(
                            label(process, 13.)
                                .font_family(MONO_FONT)
                                .w(px(process_width))
                                .flex_shrink_0(),
                        )
                        .child(label(message, 14.).flex_1().min_w_0())
                },
            )),
        )
}

const LOGS: [LogEntry; 18] = [
    (
        "10:35:01",
        "INFO",
        "application",
        "omnooth を起動しました。",
    ),
    (
        "10:35:01",
        "INFO",
        "settings",
        "設定を読み込みました。テーマ: システムに合わせる",
    ),
    (
        "10:35:02",
        "INFO",
        "projects.scan",
        "プロジェクトの一覧を更新しました。3 件のプロジェクトが見つかりました。",
    ),
    (
        "10:36:12",
        "INFO",
        "backup.create",
        "Eku のバックアップを開始しました。",
    ),
    (
        "10:36:24",
        "INFO",
        "backup.create",
        "バックアップが完了しました。保存先: ~/omnooth/backups/Eku",
    ),
    (
        "10:40:08",
        "INFO",
        "booth.receive",
        "BOOTH Assets の取り込みを受け付けました。",
    ),
    (
        "10:40:09",
        "INFO",
        "metadata.fetch",
        "商品情報を取得しました。",
    ),
    (
        "10:40:11",
        "WARN",
        "download",
        "ダウンロードリンクの有効期限が切れています。新しいリンクの受信を待っています。",
    ),
    (
        "10:41:03",
        "INFO",
        "booth.receive",
        "新しいリンクを受信しました。ダウンロードを再開します。",
    ),
    (
        "10:41:18",
        "INFO",
        "download",
        "ダウンロードが完了しました。ファイル: Eku_Outfit.zip",
    ),
    (
        "10:41:19",
        "INFO",
        "library.register",
        "原本を保存しました。版: 2026-11-19@8f3a21c0",
    ),
    (
        "10:41:19",
        "INFO",
        "archive",
        "ZIP の自動展開は OFF です。原本を保存して取り込みを完了しました。",
    ),
    (
        "10:42:02",
        "INFO",
        "install.plan",
        "Outfit.unitypackage の変更内容と競合を確認しました。",
    ),
    (
        "10:42:03",
        "INFO",
        "backup.create",
        "適用前のバックアップを作成しました。",
    ),
    (
        "10:42:05",
        "ERROR",
        "unity.connect",
        "Unity に接続できませんでした。対象のプロジェクトを Unity で開いてください。",
    ),
    (
        "10:43:10",
        "INFO",
        "unity.connect",
        "Unity Editor への接続を確認しました。",
    ),
    (
        "10:43:12",
        "INFO",
        "unity.import",
        "Outfit.unitypackage を Eku に適用しています。",
    ),
    (
        "10:43:18",
        "INFO",
        "install.verify",
        "実ファイルの照合が完了しました。適用履歴を保存しました。",
    ),
];

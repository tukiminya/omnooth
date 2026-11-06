import { test, expect, type Page } from '@playwright/test';
import type { DeepLinkStatus } from '../src/contracts';

const pageErrors = new WeakMap<Page, string[]>();
test.beforeEach(({ page }) => {
  const errors: string[] = [];
  pageErrors.set(page, errors);
  page.on('pageerror', error => errors.push(error.message));
});
test.afterEach(({ page }) => expect(pageErrors.get(page)).toEqual([]));

// Transport fixtures are confined to tests; production never seeds sample projects.
const snapshot = {
  projects: [{ id: 'project-a', name: 'Fixture project', path: '/tmp/fixture-unity-project', unityVersion: '2022.3.22f1', lastUsedAt: null }],
  items: [{
    id: 'package-a', title: 'Test distribution', versions: [
      { id: 'version-a', label: '2026-10-27@aaaaaaaa', versionDate: '2026-10-27', contentHash: 'a'.repeat(64), originalFilename: 'Distribution.zip', state: 'extracted', files: [
        { id: 'file-a', relativePath: 'Avatar.unitypackage', kind: 'unitypackage', sizeBytes: 120, contentHash: 'b'.repeat(64) },
        { id: 'file-b', relativePath: 'Quest.unitypackage', kind: 'unitypackage', sizeBytes: 130, contentHash: 'c'.repeat(64) },
      ] },
      { id: 'version-b', label: '2026-10-26@dddddddd', versionDate: '2026-10-26', contentHash: 'd'.repeat(64), originalFilename: 'Distribution.zip', state: 'stored', files: [] },
    ],
  }],
  settings: { language: 'ja', libraryPath: '/tmp/fixture-library', extractArchives: false, keepOldVersions: true },
};

async function installTransportFixture(page: Page, coldReceipt = false, initialImport: DeepLinkStatus['lastImport'] = null) {
  await page.addInitScript(({ initial, coldReceipt, initialImport }) => {
    const state = {
      snapshot: initial,
      commands: [] as string[],
      deepLink: {
        scheme: 'booth-library-manager', registered: false,
        receivedCount: coldReceipt ? 1 : 0,
        lastReceipt: coldReceipt ? { boothItemId: '42', variationId: null, filename: 'Cold.zip', sourceIdentity: 'cold-source' } : null,
        lastError: null,
        lastImport: initialImport,
        activeCount: initialImport ? 1 : 0,
      },
      reportedErrors: [] as Array<{ operation: string; message: string }>,
      getSnapshotFailures: 0,
      inspectFailures: 0,
      holdInspection: false,
      releaseInspection: null as (() => void) | null,
      logs: {
        sessionId: 'session-fixture-01',
        entries: [
          { id: 1, timestamp: '2026-10-28T10:00:00Z', level: 'info', operation: 'startup', message: 'Application started', correlationId: null },
          { id: 2, timestamp: '2026-10-28T10:00:02Z', level: 'warn', operation: 'importArchive', message: 'Archive retained without extraction', correlationId: 'corr-warn' },
          { id: 3, timestamp: '2026-10-28T10:00:03Z', level: 'error', operation: 'parseArchive', message: '<img src=x onerror=alert(1)>', correlationId: 'corr-error' },
        ],
      },
      getLogsCalls: 0,
      logsFail: false,
    };
    const callbacks = new Map<number, (event: unknown) => void>();
    const eventHandlers = new Map<number, number>();
    let callbackId = 0;
    let listenerId = 0;
    Object.assign(state, {
      emitDeepLink(status: typeof state.deepLink) {
        state.deepLink = status;
        for (const [id, handler] of eventHandlers) callbacks.get(handler)?.({ event: 'deep-link-status-changed', id, payload: status });
      },
      listenerCount: () => eventHandlers.size,
    });
    Object.assign(window, {
      isTauri: true,
      __workflowFixture: state,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
      __TAURI_INTERNALS__: {
        transformCallback(callback: (event: unknown) => void) { const id = ++callbackId; callbacks.set(id, callback); return id; },
        unregisterCallback(id: number) { callbacks.delete(id); },
        async invoke(name: string, args: { eventId?: number; handler?: number; operation?: string; message?: string; options?: { multiple?: boolean }; command?: { type: string; input?: any } }) {
          if (name === 'plugin:event|listen') { const id = ++listenerId; eventHandlers.set(id, args.handler!); return id; }
          if (name === 'plugin:event|unlisten') { eventHandlers.delete(args.eventId!); return; }
          if (name === 'get_deep_link_status') return structuredClone(state.deepLink);
          if (name === 'register_deep_link') { state.commands.push(name); state.deepLink.registered = true; return structuredClone(state.deepLink); }
          if (name === 'report_client_error') { state.reportedErrors.push({ operation: args.operation!, message: args.message! }); return; }
          if (name === 'get_logs') {
            state.getLogsCalls++;
            if (state.logsFail) throw new Error('private logging transport detail');
            return structuredClone(state.logs);
          }
          if (name === 'open_manual') { state.commands.push(name); return; }
          if (name === 'plugin:dialog|open') return args.options?.multiple ? ['/tmp/Stored.7z', '/tmp/Invalid.zip'] : '/tmp/new-library';
          if (name !== 'dispatch' || !args.command) throw new Error('Unexpected test transport');
          const { type, input } = args.command;
          state.commands.push(type);
          if (type === 'getSnapshot') {
            if (state.getSnapshotFailures > 0) {
              state.getSnapshotFailures--;
              throw new Error('SNAPSHOT_TEMPORARY_FAILURE');
            }
            return structuredClone(state.snapshot);
          }
          if (type === 'updateSettings') { state.snapshot.settings = input.settings; return input.settings; }
          if (type === 'inspectArchive') {
            if (state.holdInspection) await new Promise<void>((resolve) => { state.releaseInspection = resolve; });
            if (state.inspectFailures > 0) { state.inspectFailures--; throw new Error('ARCHIVE_INVALID'); }
            return { versionId: input.versionId, entries: [
              { id: 'zip-a', relativePath: 'nested/Avatar.unitypackage', kind: 'unitypackage', sizeBytes: 120, encrypted: false },
              { id: 'zip-b', relativePath: 'nested/Quest.unitypackage', kind: 'unitypackage', sizeBytes: 130, encrypted: false },
              { id: 'zip-c', relativePath: 'readme.txt', kind: 'file', sizeBytes: 20, encrypted: false },
              { id: 'zip-d', relativePath: 'image.png', kind: 'file', sizeBytes: 100, encrypted: false },
              { id: 'zip-e', relativePath: 'Locked.unitypackage', kind: 'unitypackage', sizeBytes: 100, encrypted: true },
            ] };
          }
          if (type === 'importArchive') {
            if (input.originalFilename === 'Invalid.zip') throw 'ARCHIVE_FORMAT_MISMATCH';
            return { item: state.snapshot.items[0], reused: true, warnings: ['EXTRACTION_UNSUPPORTED_STORED_ONLY'] };
          }
          throw new Error('Unhandled test command ' + type);
        },
      },
    });
  }, { initial: snapshot, coldReceipt, initialImport });
}

test('browser preview has an empty library and explains native file selection', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'プロジェクト', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'ログ', exact: true }).click();
  const logs = page.getByRole('dialog', { name: 'アプリログ' });
  await expect(logs.getByRole('status')).toContainText('ログはありません。');
  await logs.getByRole('button', { name: '閉じる' }).click();
  await page.getByRole('button', { name: '既存プロジェクトを紐づける', exact: true }).first().click();
  await expect(page.getByRole('alert')).toContainText('デスクトップアプリ');
  await expect(page.locator('.project-card')).toHaveCount(0);
});

test('unitypackages start unchecked, follow explicit order, and reset on version change', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'ライブラリ', exact: true }).click();
  await page.getByRole('button', { name: /Test distribution/ }).click();
  const checkboxes = page.locator('.unity-file input');
  await expect(checkboxes).toHaveCount(2);
  await expect(checkboxes.nth(0)).not.toBeChecked();
  await expect(checkboxes.nth(1)).not.toBeChecked();
  await checkboxes.nth(0).check();
  await checkboxes.nth(1).check();
  await page.getByRole('button', { name: '上へ移動' }).nth(1).click();
  await expect(page.locator('.selection-item').first()).toContainText('Quest.unitypackage');
  await page.locator('.version-select-label select').selectOption('version-b');
  await expect(page.locator('.selection-item')).toHaveCount(0);
  await page.locator('.version-select-label select').selectOption('version-a');
  await expect(checkboxes.nth(0)).not.toBeChecked();
});

test('stored ZIP contents can be searched and unitypackages selected without extraction', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'ライブラリ', exact: true }).click();
  await page.getByRole('button', { name: /Test distribution/ }).click();
  await page.locator('.version-select-label select').selectOption('version-b');
  const contents = page.getByRole('region', { name: 'ZIP の収録内容' });
  await expect(contents).toBeVisible();
  await expect(contents.getByText('readme.txt', { exact: true })).toBeVisible();
  await expect(contents.getByText('image.png', { exact: true })).toBeVisible();
  const packages = page.locator('.unity-file input');
  await expect(packages).toHaveCount(3);
  await expect(packages.nth(0)).not.toBeChecked();
  await expect(packages.nth(2)).toBeDisabled();
  await packages.nth(1).check();
  await packages.nth(0).check();
  await expect(page.locator('.selection-item').first()).toContainText('Quest.unitypackage');
  await contents.getByRole('searchbox', { name: '収録パスを検索' }).fill('readme');
  await expect(contents.locator('li')).toHaveCount(1);
  await expect(contents.locator('li')).toContainText('readme.txt');
  await expect(page.locator('.right-rail')).toContainText('取り出し時に SHA-256 を確認');
  const commands: string[] = await page.evaluate(() => (window as any).__workflowFixture.commands);
  expect(commands.filter((command) => command === 'inspectArchive')).toHaveLength(1);
  expect(commands).not.toContain('extractArchive');
});

test('a failed ZIP inspection can be retried without extracting or showing an empty-package result', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.evaluate(() => { (window as any).__workflowFixture.inspectFailures = 1; });
  await page.getByRole('button', { name: 'ライブラリ', exact: true }).click();
  await page.getByRole('button', { name: /Test distribution/ }).click();
  await page.locator('.version-select-label select').selectOption('version-b');
  await expect(page.getByRole('alert')).toContainText('ZIP の中身を読み込めませんでした');
  await expect(page.getByText('この版には unitypackage がありません。', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '再試行', exact: true }).click();
  await expect(page.getByRole('region', { name: 'ZIP の収録内容' })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__workflowFixture.commands)).not.toContain('extractArchive');
});

test('a late ZIP inspection cannot replace contents after changing versions', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.evaluate(() => { (window as any).__workflowFixture.holdInspection = true; });
  await page.getByRole('button', { name: 'ライブラリ', exact: true }).click();
  await page.getByRole('button', { name: /Test distribution/ }).click();
  await page.locator('.version-select-label select').selectOption('version-b');
  await expect(page.getByRole('status')).toContainText('ZIP の中身を確認中');
  await expect.poll(() => page.evaluate(() => Boolean((window as any).__workflowFixture.releaseInspection))).toBe(true);
  await page.locator('.version-select-label select').selectOption('version-a');
  await page.evaluate(() => (window as any).__workflowFixture.releaseInspection());
  await expect(page.locator('.unity-file input')).toHaveCount(2);
  await expect(page.getByRole('region', { name: 'ZIP の収録内容' })).toHaveCount(0);
  await expect(page.locator('.selection-item')).toHaveCount(0);
});

test('settings cancel preserves persisted values and saving changes the full locale', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: '設定', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('表示言語').selectOption('en');
  await dialog.getByRole('button', { name: 'フォルダーを選択', exact: true }).click();
  await expect(dialog.getByLabel('ライブラリの保存先')).toHaveValue('/tmp/new-library');
  await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
  await page.getByRole('button', { name: '設定', exact: true }).click();
  await expect(dialog.getByLabel('表示言語')).toHaveValue('ja');
  await expect(dialog.getByLabel('ライブラリの保存先')).toHaveValue('/tmp/fixture-library');
  await dialog.getByLabel('表示言語').selectOption('en');
  await dialog.getByRole('button', { name: '保存する', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Projects', exact: true })).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  const requests = await page.evaluate(() => (window as any).__workflowFixture.commands.length);
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  expect(await page.evaluate(() => (window as any).__workflowFixture.commands.length)).toBe(requests);
});

test('settings tabs keep exactly one active button and preserve unsaved changes', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: '設定', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const general = dialog.getByRole('tab', { name: '一般', exact: true });
  const history = dialog.getByRole('tab', { name: 'バージョン履歴', exact: true });
  await dialog.getByLabel('表示言語').selectOption('en');
  for (let count = 0; count < 3; count++) {
    await history.click();
    await expect(history).toHaveAttribute('aria-selected', 'true');
    await expect(general).toHaveAttribute('aria-selected', 'false');
    await expect(dialog.locator('.settings-category.active')).toHaveCount(1);
    await expect(dialog.getByRole('button', { name: '保存する', exact: true })).toHaveCount(1);
    await general.click();
    await expect(general).toHaveAttribute('aria-selected', 'true');
    await expect(dialog.getByLabel('表示言語')).toHaveValue('en');
    await expect(dialog.locator('.settings-category.active')).toHaveCount(1);
  }
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await page.getByRole('button', { name: '設定', exact: true }).click();
  await expect(dialog.getByLabel('表示言語')).toHaveValue('ja');
});

test('registration can replace an existing receiver and survives changing settings tabs', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await expect(page.getByRole('button', { name: '連携', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '設定', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const register = dialog.getByRole('button', { name: 'BOOTH リンクの受信先に設定', exact: true });
  await register.click();
  await expect(dialog.locator('.deep-link-status')).toContainText('omnooth が BOOTH リンクの受信先です。');
  await dialog.getByRole('tab', { name: 'バージョン履歴', exact: true }).click();
  await dialog.getByRole('tab', { name: '一般', exact: true }).click();
  await expect(dialog.locator('.deep-link-status')).toContainText('omnooth が BOOTH リンクの受信先です。');
  expect(await page.evaluate(() => (window as any).__workflowFixture.commands.filter((name: string) => name === 'register_deep_link').length)).toBe(1);
});

test('manual opens the requested documentation URL in a new browser tab', async ({ page, context }) => {
  await context.route('https://omnooth.tuki.dev/docs', route => route.fulfill({ status: 200, body: 'Manual test page' }));
  await page.goto('/');
  const opened = context.waitForEvent('page');
  await page.getByRole('button', { name: 'マニュアル', exact: true }).click();
  const manual = await opened;
  await expect(manual).toHaveURL('https://omnooth.tuki.dev/docs');
  await manual.close();
});

test('logs open above Manual, filter by level, refresh, and poll plain-text entries', async ({ page }, testInfo) => {
  await installTransportFixture(page);
  await page.goto('/');
  const utilityButtons = page.locator('.nav-bottom .nav-item');
  await expect(utilityButtons).toHaveText(['ログ', 'マニュアル', '設定']);
  await utilityButtons.nth(0).click();
  const dialog = page.getByRole('dialog', { name: 'アプリログ' });
  await expect(dialog.getByText('Application started')).toBeVisible();
  await expect(dialog.getByText('session-fixture-01')).toBeVisible();
  await expect(dialog.locator('.log-entry')).toHaveCount(3);
  await dialog.getByLabel('レベル').selectOption('error');
  await expect(dialog.locator('.log-entry')).toHaveCount(1);
  await expect(dialog.getByText('<img src=x onerror=alert(1)>', { exact: true })).toBeVisible();
  await expect(dialog.locator('img')).toHaveCount(0);
  await dialog.getByLabel('レベル').selectOption('all');
  await dialog.screenshot({ path: testInfo.outputPath('logs-dialog.png') });

  const callsBeforeRefresh = await page.evaluate(() => (window as any).__workflowFixture.getLogsCalls);
  await dialog.getByRole('button', { name: '更新', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__workflowFixture.getLogsCalls)).toBeGreaterThan(callsBeforeRefresh);
  await page.evaluate(() => {
    (window as any).__workflowFixture.logs.entries = [
      { id: 4, timestamp: '2026-10-28T10:00:04Z', level: 'info', operation: 'syncLibrary', message: 'A new operation was recorded', correlationId: null },
    ];
  });
  await expect(dialog.getByText('A new operation was recorded')).toBeVisible({ timeout: 4000 });
  await dialog.getByRole('button', { name: '閉じる' }).click();
  await expect(dialog).not.toBeVisible();
});

test('logs show empty and localized error states with retry', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.evaluate(() => { (window as any).__workflowFixture.logs.entries = []; });
  await page.getByRole('button', { name: 'ログ', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'アプリログ' });
  await expect(dialog.getByRole('status')).toContainText('ログはありません。');
  await page.evaluate(() => { (window as any).__workflowFixture.logsFail = true; });
  await dialog.getByRole('button', { name: '更新', exact: true }).click();
  await expect(dialog.getByRole('alert')).toHaveText('ログを読み込めませんでした。');
  await expect(dialog).not.toContainText('private logging transport detail');
  expect(await page.evaluate(() => (window as any).__workflowFixture.reportedErrors.length)).toBe(0);
  await page.evaluate(() => { (window as any).__workflowFixture.logsFail = false; });
  await dialog.getByRole('button', { name: '更新', exact: true }).click();
  await expect(dialog.getByRole('alert')).toHaveCount(0);
  await expect(dialog.getByRole('status')).toContainText('ログはありません。');
});

test('startup and live deep-link receipts show filenames without claiming import', async ({ page }) => {
  await installTransportFixture(page, true);
  await page.goto('/');
  await expect(page.getByRole('status')).toContainText('BOOTH からリンクを受信しました：Cold.zip');
  await expect.poll(() => page.evaluate(() => (window as any).__workflowFixture.listenerCount())).toBe(1);
  await page.evaluate(() => (window as any).__workflowFixture.emitDeepLink({
    scheme: 'booth-library-manager', registered: true, receivedCount: 2,
    lastReceipt: { boothItemId: '43', variationId: null, filename: 'Live.zip', sourceIdentity: 'live-source' }, lastError: null,
  }));
  await expect(page.getByRole('status')).toContainText('BOOTH からリンクを受信しました：Live.zip');
  await expect(page.locator('body')).not.toContainText('ライブラリに取り込みました');
});

test('deep-link import progress and failure are announced once and link to logs', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await expect.poll(() => page.evaluate(() => (window as any).__workflowFixture.listenerCount())).toBe(1);
  const emit = async (state: NonNullable<DeepLinkStatus['lastImport']>['state'], errorCode: string | null = null) => page.evaluate(({ state, errorCode }) => {
    const fixture = (window as any).__workflowFixture;
    fixture.emitDeepLink({
      ...fixture.deepLink, receivedCount: 1, lastReceipt: null, lastError: null, activeCount: 1,
      lastImport: { jobId: 27, filename: 'Avatar.zip', state, errorCode, itemId: null },
    });
  }, { state, errorCode });

  await emit('queued');
  await expect(page.getByRole('status')).toContainText('BOOTH からのリンクを待機中：Avatar.zip');
  await emit('downloading');
  await expect(page.getByRole('status')).toContainText('BOOTH からダウンロード中：Avatar.zip');
  await emit('awaitingUrl', 'DOWNLOAD_URL_EXPIRED');
  await expect(page.getByRole('alert')).toContainText('BOOTH から新しいリンクを開いてください');
  await emit('failed', 'DOWNLOAD_REJECTED');
  await expect(page.getByRole('alert')).toContainText('受信に失敗しました：Avatar.zip（DOWNLOAD_REJECTED）');
  const snapshotCallsBeforeRepeat = await page.evaluate(() => (window as any).__workflowFixture.commands.filter((name: string) => name === 'getSnapshot').length);
  await page.getByRole('button', { name: '閉じる', exact: true }).click();
  await emit('failed', 'DOWNLOAD_REJECTED');
  await expect(page.getByRole('alert')).toHaveCount(0);
  await emit('queued');
  await expect(page.getByRole('status')).toContainText('BOOTH からのリンクを待機中：Avatar.zip');
  await emit('downloading');
  await emit('failed', 'DOWNLOAD_REJECTED');
  await expect(page.getByRole('button', { name: 'ログを開く', exact: true })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(1);
  expect(await page.evaluate(() => (window as any).__workflowFixture.commands.filter((name: string) => name === 'getSnapshot').length)).toBe(snapshotCallsBeforeRepeat);

  await page.getByRole('button', { name: 'ログを開く', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'アプリログ' })).toBeVisible();
});

test('deep-link success refreshes and selects the imported item', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await expect.poll(() => page.evaluate(() => (window as any).__workflowFixture.listenerCount())).toBe(1);
  await page.evaluate(() => {
    const fixture = (window as any).__workflowFixture;
    const emit = (state: string) => fixture.emitDeepLink({
      ...fixture.deepLink, receivedCount: 1, lastReceipt: null, lastError: null, activeCount: 1,
      lastImport: { jobId: 31, filename: 'Avatar.zip', state, errorCode: null, itemId: state === 'succeeded' ? 'package-a' : null },
    });
    emit('queued'); emit('downloading'); emit('importing'); emit('succeeded');
  });
  await expect(page.getByRole('heading', { name: 'ライブラリ', exact: true })).toBeVisible();
  await expect(page.locator('.item-row.selected')).toContainText('Test distribution');
  await expect(page.getByRole('status')).toContainText('ライブラリに追加しました：Avatar.zip');
  expect(await page.evaluate(() => (window as any).__workflowFixture.commands.filter((name: string) => name === 'getSnapshot').length)).toBeGreaterThan(1);
});

test('a failed success refresh can retry when the same succeeded state is repeated', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'プロジェクト', exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__workflowFixture.listenerCount())).toBe(1);
  await page.evaluate(() => { (window as any).__workflowFixture.getSnapshotFailures = 1; });
  const emitSuccess = () => page.evaluate(() => {
    const fixture = (window as any).__workflowFixture;
    fixture.emitDeepLink({
      ...fixture.deepLink, receivedCount: 1, lastReceipt: null, lastError: null, activeCount: 0,
      lastImport: { jobId: 35, filename: 'Retry.zip', state: 'succeeded', errorCode: null, itemId: 'package-a' },
    });
  });
  await emitSuccess();
  await expect(page.getByRole('alert')).toContainText('SNAPSHOT_TEMPORARY_FAILURE');
  await emitSuccess();
  await expect(page.getByRole('heading', { name: 'ライブラリ', exact: true })).toBeVisible();
  await expect(page.locator('.item-row.selected')).toContainText('Test distribution');
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('ライブラリに追加しました：Retry.zip');
});

test('startup success refreshes the library data without taking over the current page', async ({ page }) => {
  await installTransportFixture(page, false, { jobId: 32, filename: 'Earlier.zip', state: 'succeeded', errorCode: null, itemId: 'package-a' });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'プロジェクト', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'ライブラリ', exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__workflowFixture.commands.filter((name: string) => name === 'getSnapshot').length)).toBeGreaterThan(1);
  await page.getByRole('button', { name: 'ライブラリ', exact: true }).click();
  await expect(page.locator('.item-row.selected')).toContainText('Test distribution');
});

test('cold start for a newly received link already succeeded opens the imported item', async ({ page }) => {
  await installTransportFixture(page, true, { jobId: 36, filename: 'Cold.zip', state: 'succeeded', errorCode: null, itemId: 'package-a' });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'ライブラリ', exact: true })).toBeVisible();
  await expect(page.locator('.item-row.selected')).toContainText('Test distribution');
  await expect(page.getByRole('status')).toContainText('ライブラリに追加しました：Cold.zip');
});

test('window errors and unhandled rejections are reported without recursive reporting', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.evaluate(() => {
    window.dispatchEvent(new ErrorEvent('error', { message: 'fixture window error' }));
    const rejection = new Event('unhandledrejection');
    Object.defineProperty(rejection, 'reason', { value: new Error('fixture promise rejection') });
    window.dispatchEvent(rejection);
  });
  await expect.poll(() => page.evaluate(() => (window as any).__workflowFixture.reportedErrors.length)).toBe(2);
  const reported = await page.evaluate(() => (window as any).__workflowFixture.reportedErrors);
  expect(reported.map((entry: { operation: string }) => entry.operation).sort()).toEqual(['unhandledrejection', 'window.error']);
});

test('batch import retains successes and displays per-file failures and warnings', async ({ page }) => {
  await installTransportFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'ファイルを追加', exact: true }).first().click();
  await expect(page.getByRole('alert')).toContainText('取り込み 1 件・失敗 1 件・注意 1 件');
  await expect(page.getByRole('alert')).toContainText('Invalid.zip');
  await expect(page.getByRole('alert')).toContainText('原本を保存しました');
});

for (const width of [1280, 1440, 1920]) {
  test(`layout and static assets remain visible at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 1080 });
    await installTransportFixture(page);
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'プロジェクト', exact: true })).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    await page.locator('img').evaluateAll(images => Promise.all(images.map(img => (img as HTMLImageElement).decode())));
    const geometry = await page.evaluate(() => ({
      overflow: document.documentElement.scrollWidth > window.innerWidth,
      images: [...document.images].map((img) => ({
        source: img.getAttribute('src'), loaded: img.complete && img.naturalWidth > 0,
        width: img.getBoundingClientRect().width, height: img.getBoundingClientRect().height,
      })),
    }));
    expect(geometry.overflow).toBe(false);
    expect(geometry.images.filter(img => !img.loaded || img.width <= 0 || img.height <= 0)).toEqual([]);
    await page.screenshot({ path: test.info().outputPath(`projects-${width}.png`) });
  });
}

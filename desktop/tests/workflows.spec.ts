import { test, expect, type Page } from '@playwright/test';

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

async function installTransportFixture(page: Page, coldReceipt = false) {
  await page.addInitScript(({ initial, coldReceipt }) => {
    const state = {
      snapshot: initial,
      commands: [] as string[],
      deepLink: {
        scheme: 'booth-library-manager', registered: false,
        receivedCount: coldReceipt ? 1 : 0,
        lastReceipt: coldReceipt ? { boothItemId: '42', variationId: null, filename: 'Cold.zip', sourceIdentity: 'cold-source' } : null,
        lastError: null,
      },
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
        async invoke(name: string, args: { eventId?: number; handler?: number; options?: { multiple?: boolean }; command?: { type: string; input?: any } }) {
          if (name === 'plugin:event|listen') { const id = ++listenerId; eventHandlers.set(id, args.handler!); return id; }
          if (name === 'plugin:event|unlisten') { eventHandlers.delete(args.eventId!); return; }
          if (name === 'get_deep_link_status') return structuredClone(state.deepLink);
          if (name === 'register_deep_link') { state.commands.push(name); state.deepLink.registered = true; return structuredClone(state.deepLink); }
          if (name === 'open_manual') { state.commands.push(name); return; }
          if (name === 'plugin:dialog|open') return args.options?.multiple ? ['/tmp/Stored.7z', '/tmp/Invalid.zip'] : '/tmp/new-library';
          if (name !== 'dispatch' || !args.command) throw new Error('Unexpected test transport');
          const { type, input } = args.command;
          state.commands.push(type);
          if (type === 'getSnapshot') return structuredClone(state.snapshot);
          if (type === 'updateSettings') { state.snapshot.settings = input.settings; return input.settings; }
          if (type === 'importArchive') {
            if (input.originalFilename === 'Invalid.zip') throw 'ARCHIVE_FORMAT_MISMATCH';
            return { item: state.snapshot.items[0], reused: true, warnings: ['EXTRACTION_UNSUPPORTED_STORED_ONLY'] };
          }
          throw new Error('Unhandled test command ' + type);
        },
      },
    });
  }, { initial: snapshot, coldReceipt });
}

test('browser preview has an empty library and explains native file selection', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'プロジェクト', exact: true })).toBeVisible();
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

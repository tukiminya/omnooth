package runlog

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"
)

func TestOpenCreatesPrivateUniqueLogFiles(t *testing.T) {
	home := t.TempDir()
	now := time.Date(2026, time.August, 15, 12, 34, 56, 123456789, time.Local)

	first, err := Open(home, now)
	if err != nil {
		t.Fatal(err)
	}
	second, err := Open(home, now)
	if err != nil {
		first.Close()
		t.Fatal(err)
	}
	if first.Path == second.Path {
		t.Fatal("concurrent sessions reused the same log path")
	}
	if !regexp.MustCompile(`log_\d{8}_\d{6}\.\d{9}\.txt$`).MatchString(first.Path) {
		t.Fatalf("unexpected log filename: %s", first.Path)
	}

	first.Logger.Info("test event", "event", "test", "value", "ok")
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	if err := second.Close(); err != nil {
		t.Fatal(err)
	}

	info, err := os.Stat(first.Path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("log permissions = %o, want 600", info.Mode().Perm())
	}
	directoryInfo, err := os.Stat(filepath.Dir(first.Path))
	if err != nil {
		t.Fatal(err)
	}
	if directoryInfo.Mode().Perm() != 0o700 {
		t.Fatalf("log directory permissions = %o, want 700", directoryInfo.Mode().Perm())
	}
	content, err := os.ReadFile(first.Path)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(content), `event=test`) {
		t.Fatalf("log event missing: %s", content)
	}
}

func TestLoggerRedactsSensitiveValues(t *testing.T) {
	session, err := Open(t.TempDir(), time.Now())
	if err != nil {
		t.Fatal(err)
	}
	session.Logger.Error("request https://download.booth.pm/file?signature=secret failed",
		"uri", "booth-library-manager://item-import?dlurl=secret",
		"header", "Authorization: bearer-secret",
	)
	if err := session.Close(); err != nil {
		t.Fatal(err)
	}
	content, err := os.ReadFile(session.Path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(content)
	for _, secret := range []string{"signature=secret", "dlurl=secret", "bearer-secret"} {
		if strings.Contains(text, secret) {
			t.Fatalf("log contains secret %q: %s", secret, text)
		}
	}
	if !strings.Contains(text, "[redacted") {
		t.Fatalf("log does not show redaction marker: %s", text)
	}
}

func TestErrorAttrsIncludeDeepestCauseWithoutURL(t *testing.T) {
	root := errors.New("request https://booth.pm/file?token=secret failed")
	attributes := ErrorAttrs(fmt.Errorf("outer operation: %w", root))
	text := fmt.Sprint(attributes)
	if strings.Contains(text, "token=secret") {
		t.Fatalf("error attributes contain URL: %s", text)
	}
	if !strings.Contains(text, "root_cause") || !strings.Contains(text, "[redacted-url]") {
		t.Fatalf("error attributes lack safe root cause: %s", text)
	}
}

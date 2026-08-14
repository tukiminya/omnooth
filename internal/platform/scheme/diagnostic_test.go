package scheme

import (
	"os"
	"path/filepath"
	"testing"
)

func TestHandlerDiagnosticRoundTrip(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, "config"))

	if err := RecordHandlerDiagnostic("failed\nforged", "invalid_import_uri\r\nforged"); err != nil {
		t.Fatal(err)
	}
	diagnostic, found, err := ReadHandlerDiagnostic()
	if err != nil {
		t.Fatal(err)
	}
	if !found {
		t.Fatal("expected a diagnostic record")
	}
	if diagnostic.State != "failed forged" || diagnostic.Detail != "invalid_import_uri  forged" {
		t.Fatalf("unexpected sanitized diagnostic: %#v", diagnostic)
	}

	filename, err := handlerDiagnosticPath()
	if err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(filename)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("diagnostic permissions = %o, want 600", info.Mode().Perm())
	}

	if err := RemoveHandlerDiagnostic(); err != nil {
		t.Fatal(err)
	}
	if _, found, err := ReadHandlerDiagnostic(); err != nil || found {
		t.Fatalf("diagnostic still exists: found=%t err=%v", found, err)
	}
}

func TestHandlerDiagnosticRedactsURLs(t *testing.T) {
	for _, value := range []string{
		"https://booth.pm/download?signature=secret",
		"booth-library-manager://item-import?dlurl=secret",
		"dlurl=secret",
	} {
		if got := diagnosticDetail(value); got != "[redacted]" {
			t.Errorf("diagnosticDetail(%q) = %q, want redacted", value, got)
		}
	}
}

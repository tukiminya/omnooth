package cmd

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestExecuteCreatesLogAndReportsPathOnFailure(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	var stdout bytes.Buffer
	var stderr bytes.Buffer

	if code := execute([]string{"unknown-command"}, &stdout, &stderr); code != 1 {
		t.Fatalf("exit code = %d, want 1", code)
	}
	logs, err := filepath.Glob(filepath.Join(home, "omnooth", "logs", "log_*.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if len(logs) != 1 {
		t.Fatalf("log files = %v, want exactly one", logs)
	}
	content, err := os.ReadFile(logs[0])
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(content), "event=process_started") || !strings.Contains(string(content), "event=command_failed") {
		t.Fatalf("required process events missing: %s", content)
	}
	if !strings.Contains(stderr.String(), "Log: "+logs[0]) {
		t.Fatalf("stderr does not identify log path: %s", stderr.String())
	}
}

func TestCommandLabelNeverIncludesArguments(t *testing.T) {
	secretURI := "booth-library-manager://item-import?dlurl=https://booth.pm/?signature=secret"
	if label := commandLabel([]string{"import", secretURI}); label != "omnooth import" {
		t.Fatalf("command label = %q", label)
	}
	if label := commandLabel([]string{"scheme", "handle", secretURI}); label != "omnooth scheme handle" {
		t.Fatalf("handler command label = %q", label)
	}
}

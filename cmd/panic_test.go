package cmd

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type panicWriter struct{}

func (panicWriter) Write([]byte) (int, error) {
	panic(errors.New("writer failed at https://booth.pm/?signature=panic-secret"))
}

func TestExecuteLogsPanicAndReturnsFailure(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	var stderr bytes.Buffer

	if code := execute([]string{"--help"}, panicWriter{}, &stderr); code != 1 {
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
	text := string(content)
	if !strings.Contains(text, "event=process_panicked") || !strings.Contains(text, "stack=") {
		t.Fatalf("panic diagnostics missing: %s", text)
	}
	if strings.Contains(text, "panic-secret") {
		t.Fatalf("panic URL leaked into log: %s", text)
	}
	if !strings.Contains(stderr.String(), "Log: "+logs[0]) {
		t.Fatalf("stderr does not identify panic log: %s", stderr.String())
	}
}

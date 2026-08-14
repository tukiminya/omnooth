package scheme

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const handlerDiagnosticFilename = "last-handler-event.json"

// HandlerDiagnostic は、無害化した状態だけを保持する。
// Custom URLや署名付きdlurlは決して含めない。
type HandlerDiagnostic struct {
	Timestamp time.Time `json:"timestamp"`
	State     string    `json:"state"`
	Detail    string    `json:"detail,omitempty"`
}

func RecordHandlerDiagnostic(state, detail string) error {
	filename, err := handlerDiagnosticPath()
	if err != nil {
		return err
	}
	diagnostic := HandlerDiagnostic{
		Timestamp: time.Now(),
		State:     singleLine(state, 64),
		Detail:    diagnosticDetail(detail),
	}
	content, err := json.Marshal(diagnostic)
	if err != nil {
		return fmt.Errorf("encode URL handler diagnostic: %w", err)
	}
	if err := writeFileAtomic(filename, append(content, '\n'), 0o600); err != nil {
		return fmt.Errorf("write URL handler diagnostic: %w", err)
	}
	return nil
}

func ReadHandlerDiagnostic() (HandlerDiagnostic, bool, error) {
	filename, err := handlerDiagnosticPath()
	if err != nil {
		return HandlerDiagnostic{}, false, err
	}
	content, err := os.ReadFile(filename)
	if os.IsNotExist(err) {
		return HandlerDiagnostic{}, false, nil
	}
	if err != nil {
		return HandlerDiagnostic{}, false, fmt.Errorf("read URL handler diagnostic: %w", err)
	}
	var diagnostic HandlerDiagnostic
	if err := json.Unmarshal(content, &diagnostic); err != nil {
		return HandlerDiagnostic{}, false, fmt.Errorf("decode URL handler diagnostic: %w", err)
	}
	return diagnostic, true, nil
}

func RemoveHandlerDiagnostic() error {
	filename, err := handlerDiagnosticPath()
	if err != nil {
		return err
	}
	if err := os.Remove(filename); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove URL handler diagnostic: %w", err)
	}
	return nil
}

func handlerDiagnosticPath() (string, error) {
	config, err := os.UserConfigDir()
	if err != nil {
		return "", fmt.Errorf("find user configuration directory: %w", err)
	}
	return filepath.Join(config, "omnooth", handlerDiagnosticFilename), nil
}

func singleLine(value string, maximum int) string {
	value = strings.TrimSpace(strings.NewReplacer("\r", " ", "\n", " ").Replace(value))
	if len(value) > maximum {
		value = value[:maximum]
	}
	return value
}

func diagnosticDetail(value string) string {
	value = singleLine(value, 1024)
	lower := strings.ToLower(value)
	if strings.Contains(lower, "://") || strings.Contains(lower, "dlurl=") {
		return "[redacted]"
	}
	return value
}

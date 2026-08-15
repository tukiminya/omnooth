package runlog

import (
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"runtime/debug"
	"strings"
	"time"
)

var (
	urlPattern          = regexp.MustCompile(`(?i)\b(?:https?|booth-library-manager)://[^\s"']+`)
	dlurlPattern        = regexp.MustCompile(`(?i)(dlurl=)[^&\s]+`)
	querySecretPattern  = regexp.MustCompile(`(?i)\b(signature|token|access_token|credential|key|x-amz-[a-z-]+)=([^&\s]+)`)
	headerSecretPattern = regexp.MustCompile(`(?i)\b(authorization|cookie|set-cookie)\s*[:=]\s*[^\s,;]+`)
)

// Session owns the log file for one omnooth process.
type Session struct {
	Logger *slog.Logger
	Path   string
	file   *os.File
}

// Open creates a private, per-process text log below ~/omnooth/logs.
func Open(home string, now time.Time) (*Session, error) {
	directory := filepath.Join(home, "omnooth", "logs")
	if err := os.MkdirAll(directory, 0o700); err != nil {
		return nil, fmt.Errorf("create log directory: %w", err)
	}
	if err := os.Chmod(directory, 0o700); err != nil {
		return nil, fmt.Errorf("set log directory permissions: %w", err)
	}

	var file *os.File
	var filename string
	for attempt := 0; attempt < 1000; attempt++ {
		stamp := now.Add(time.Duration(attempt)).Format("20060102_150405.000000000")
		filename = filepath.Join(directory, "log_"+stamp+".txt")
		var err error
		file, err = os.OpenFile(filename, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
		if err == nil {
			break
		}
		if !errors.Is(err, os.ErrExist) {
			return nil, fmt.Errorf("create log file: %w", err)
		}
	}
	if file == nil {
		return nil, errors.New("create log file: timestamp collision limit reached")
	}

	handler := slog.NewTextHandler(file, &slog.HandlerOptions{
		Level: slog.LevelDebug,
		ReplaceAttr: func(_ []string, attribute slog.Attr) slog.Attr {
			if attribute.Value.Kind() == slog.KindString {
				attribute.Value = slog.StringValue(Sanitize(attribute.Value.String()))
			}
			return attribute
		},
	})
	return &Session{Logger: slog.New(handler), Path: filename, file: file}, nil
}

// Close flushes the file to disk before closing it.
func (s *Session) Close() error {
	if s == nil || s.file == nil {
		return nil
	}
	syncErr := s.file.Sync()
	closeErr := s.file.Close()
	return errors.Join(syncErr, closeErr)
}

// Sanitize removes URLs and common HTTP secrets from diagnostic text.
func Sanitize(value string) string {
	value = urlPattern.ReplaceAllString(value, "[redacted-url]")
	value = dlurlPattern.ReplaceAllString(value, "${1}[redacted]")
	value = querySecretPattern.ReplaceAllString(value, "${1}=[redacted]")
	value = headerSecretPattern.ReplaceAllString(value, "${1}=[redacted]")
	return value
}

// ErrorAttrs returns safe structured details for an error and its deepest cause.
func ErrorAttrs(err error) []any {
	if err == nil {
		return nil
	}
	root := err
	for {
		next := errors.Unwrap(root)
		if next == nil {
			break
		}
		root = next
	}
	return []any{
		slog.String("error", Sanitize(err.Error())),
		slog.String("error_type", fmt.Sprintf("%T", err)),
		slog.String("root_cause", Sanitize(root.Error())),
		slog.String("root_cause_type", fmt.Sprintf("%T", root)),
	}
}

// PanicAttrs returns a safe panic value and the current goroutine stack.
func PanicAttrs(value any) []any {
	return []any{
		slog.String("panic", Sanitize(fmt.Sprint(value))),
		slog.String("stack", strings.TrimSpace(Sanitize(string(debug.Stack())))),
	}
}

package platform

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode"

	"github.com/gofrs/flock"
	"github.com/tukiminya/omnooth/internal/importer"
)

type LocalStore struct {
	Root string
}

func NewLocalStore(root string) *LocalStore {
	return &LocalStore{Root: root}
}

func (s *LocalStore) Replace(
	ctx context.Context,
	metadata importer.ItemMetadata,
	downloadableFilename string,
	populate func(workDir, contentDir string) (bool, error),
) (importer.ImportResult, error) {
	if s.Root == "" || populate == nil {
		return importer.ImportResult{}, errors.New("library store is not configured")
	}
	if metadata.ShopID <= 0 || metadata.ItemID <= 0 || strings.TrimSpace(metadata.ShopName) == "" || strings.TrimSpace(metadata.ItemName) == "" {
		return importer.ImportResult{}, errors.New("item metadata is incomplete")
	}

	target := filepath.Join(
		s.Root,
		"items",
		strconv.FormatInt(metadata.ShopID, 10)+"_"+SanitizeComponent(metadata.ShopName),
		strconv.FormatInt(metadata.ItemID, 10)+"_"+SanitizeComponent(metadata.ItemName),
		SanitizeComponent(downloadableFilename),
	)
	lockDir := filepath.Join(s.Root, ".locks")
	if err := os.MkdirAll(lockDir, 0o700); err != nil {
		return importer.ImportResult{}, fmt.Errorf("create lock directory: %w", err)
	}
	lockName := fmt.Sprintf("%x.lock", sha256.Sum256([]byte(target)))
	fileLock := flock.New(filepath.Join(lockDir, lockName))
	locked, err := fileLock.TryLockContext(ctx, 100*time.Millisecond)
	if err != nil {
		return importer.ImportResult{}, fmt.Errorf("lock import target: %w", err)
	}
	if !locked {
		return importer.ImportResult{}, ctx.Err()
	}
	defer fileLock.Close()

	tempRoot := filepath.Join(s.Root, ".tmp")
	if err := os.MkdirAll(tempRoot, 0o700); err != nil {
		return importer.ImportResult{}, fmt.Errorf("create temporary directory: %w", err)
	}
	staging, err := os.MkdirTemp(tempRoot, "import-")
	if err != nil {
		return importer.ImportResult{}, fmt.Errorf("create import staging directory: %w", err)
	}
	defer os.RemoveAll(staging)
	workDir := filepath.Join(staging, "work")
	contentDir := filepath.Join(staging, "content")
	if err := os.MkdirAll(workDir, 0o700); err != nil {
		return importer.ImportResult{}, fmt.Errorf("create import work directory: %w", err)
	}
	if err := os.MkdirAll(contentDir, 0o755); err != nil {
		return importer.ImportResult{}, fmt.Errorf("create import content directory: %w", err)
	}

	extracted, err := populate(workDir, contentDir)
	if err != nil {
		return importer.ImportResult{}, err
	}
	if err := ctx.Err(); err != nil {
		return importer.ImportResult{}, err
	}
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return importer.ImportResult{}, fmt.Errorf("create item directory: %w", err)
	}
	if err := swapDirectory(target, contentDir, staging); err != nil {
		return importer.ImportResult{}, err
	}
	return importer.ImportResult{Destination: target, Extracted: extracted}, nil
}

func swapDirectory(target, content, staging string) error {
	backup := filepath.Join(staging, "previous")
	_, err := os.Lstat(target)
	switch {
	case err == nil:
		if err := os.Rename(target, backup); err != nil {
			return fmt.Errorf("stage previous import: %w", err)
		}
		if err := os.Rename(content, target); err != nil {
			restoreErr := os.Rename(backup, target)
			if restoreErr != nil {
				return fmt.Errorf("activate import: %w (restore also failed: %v)", err, restoreErr)
			}
			return fmt.Errorf("activate import: %w", err)
		}
		// 新しい内容はすでに有効なので、バックアップ削除はベストエフォートとする。
		// 後片付けだけの失敗を取込失敗として報告しない。
		_ = os.RemoveAll(backup)
		return nil
	case errors.Is(err, os.ErrNotExist):
		if err := os.Rename(content, target); err != nil {
			return fmt.Errorf("activate import: %w", err)
		}
		return nil
	default:
		return fmt.Errorf("inspect import target: %w", err)
	}
}

var windowsReservedNames = map[string]struct{}{
	"CON": {}, "PRN": {}, "AUX": {}, "NUL": {},
	"COM1": {}, "COM2": {}, "COM3": {}, "COM4": {}, "COM5": {}, "COM6": {}, "COM7": {}, "COM8": {}, "COM9": {},
	"LPT1": {}, "LPT2": {}, "LPT3": {}, "LPT4": {}, "LPT5": {}, "LPT6": {}, "LPT7": {}, "LPT8": {}, "LPT9": {},
}

// SanitizeComponent は、複数OSで安全に扱える単一のパス要素を生成する。
func SanitizeComponent(value string) string {
	value = strings.TrimSpace(value)
	var builder strings.Builder
	lastUnderscore := false
	for _, r := range value {
		invalid := unicode.IsControl(r) || strings.ContainsRune(`/\\:*?"<>|`, r)
		if invalid {
			if !lastUnderscore {
				builder.WriteByte('_')
				lastUnderscore = true
			}
			continue
		}
		builder.WriteRune(r)
		lastUnderscore = false
	}
	cleaned := strings.Trim(builder.String(), " .")
	if cleaned == "" || cleaned == "." || cleaned == ".." {
		cleaned = "_"
	}
	base := cleaned
	if dot := strings.IndexByte(base, '.'); dot >= 0 {
		base = base[:dot]
	}
	if _, reserved := windowsReservedNames[strings.ToUpper(base)]; reserved {
		cleaned = "_" + cleaned
	}
	return cleaned
}

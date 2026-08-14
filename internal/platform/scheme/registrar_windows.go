//go:build windows

package scheme

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/sys/windows/registry"
)

const windowsSchemeKey = `Software\Classes\booth-library-manager`

type windowsRegistrar struct {
	executable string
}

func NewRegistrar() (Registrar, error) {
	config, err := os.UserConfigDir()
	if err != nil {
		return nil, fmt.Errorf("find user configuration directory: %w", err)
	}
	return &windowsRegistrar{executable: filepath.Join(config, "omnooth", "bin", "omnooth.exe")}, nil
}

func (r *windowsRegistrar) Install() error {
	if err := installExecutable(r.executable); err != nil {
		return err
	}
	root, _, err := registry.CreateKey(registry.CURRENT_USER, windowsSchemeKey, registry.ALL_ACCESS)
	if err != nil {
		return fmt.Errorf("create URL scheme registry key: %w", err)
	}
	defer root.Close()
	if err := root.SetStringValue("", "URL:BOOTH Library Manager Protocol"); err != nil {
		return fmt.Errorf("set URL scheme description: %w", err)
	}
	if err := root.SetStringValue("URL Protocol", ""); err != nil {
		return fmt.Errorf("mark URL protocol: %w", err)
	}
	commandKey, _, err := registry.CreateKey(root, `shell\open\command`, registry.ALL_ACCESS)
	if err != nil {
		return fmt.Errorf("create URL scheme command: %w", err)
	}
	defer commandKey.Close()
	command := fmt.Sprintf(`"%s" scheme handle "%%1"`, r.executable)
	if err := commandKey.SetStringValue("", command); err != nil {
		return fmt.Errorf("set URL scheme command: %w", err)
	}
	return nil
}

func (r *windowsRegistrar) Status() (Status, error) {
	key, err := registry.OpenKey(registry.CURRENT_USER, windowsSchemeKey+`\shell\open\command`, registry.QUERY_VALUE)
	if errors.Is(err, registry.ErrNotExist) {
		return Status{Detail: "not installed"}, nil
	}
	if err != nil {
		return Status{}, fmt.Errorf("read URL scheme command: %w", err)
	}
	defer key.Close()
	command, _, err := key.GetStringValue("")
	if err != nil {
		return Status{}, fmt.Errorf("read URL scheme command: %w", err)
	}
	_, statErr := os.Stat(r.executable)
	installed := statErr == nil
	expected := fmt.Sprintf(`"%s" scheme handle "%%1"`, r.executable)
	active := installed && strings.EqualFold(command, expected)
	return Status{Installed: installed, Active: active, Detail: command}, nil
}

func (r *windowsRegistrar) Uninstall() error {
	keys := []string{
		windowsSchemeKey + `\shell\open\command`,
		windowsSchemeKey + `\shell\open`,
		windowsSchemeKey + `\shell`,
		windowsSchemeKey,
	}
	for _, key := range keys {
		if err := registry.DeleteKey(registry.CURRENT_USER, key); err != nil && !errors.Is(err, registry.ErrNotExist) {
			return fmt.Errorf("remove URL scheme registry key: %w", err)
		}
	}
	if err := os.Remove(r.executable); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove handler binary: %w", err)
	}
	return RemoveHandlerDiagnostic()
}

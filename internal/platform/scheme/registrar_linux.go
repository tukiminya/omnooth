//go:build linux

package scheme

import (
	"bufio"
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

const (
	desktopFilename = "omnooth-url-handler.desktop"
	schemeMIMEType  = "x-scheme-handler/booth-library-manager"
)

type linuxRegistrar struct {
	executable  string
	desktopFile string
	configHome  string
	dataHome    string
}

func NewRegistrar() (Registrar, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, fmt.Errorf("find home directory: %w", err)
	}
	dataHome := os.Getenv("XDG_DATA_HOME")
	if dataHome == "" {
		dataHome = filepath.Join(home, ".local", "share")
	}
	configHome := os.Getenv("XDG_CONFIG_HOME")
	if configHome == "" {
		configHome = filepath.Join(home, ".config")
	}
	return &linuxRegistrar{
		executable:  filepath.Join(home, ".local", "lib", "omnooth", "omnooth"),
		desktopFile: filepath.Join(dataHome, "applications", desktopFilename),
		configHome:  configHome,
		dataHome:    dataHome,
	}, nil
}

func (r *linuxRegistrar) Install() error {
	if _, err := exec.LookPath("xdg-mime"); err != nil {
		return fmt.Errorf("xdg-mime is required to register the URL scheme: %w", err)
	}
	if err := installExecutable(r.executable); err != nil {
		return err
	}
	desktop := fmt.Sprintf(`[Desktop Entry]
Version=1.0
Type=Application
Name=omnooth URL Handler
NoDisplay=true
Terminal=false
Exec=%s scheme handle %%u
MimeType=%s;
`, desktopQuote(r.executable), schemeMIMEType)
	if err := writeFileAtomic(r.desktopFile, []byte(desktop), 0o644); err != nil {
		return fmt.Errorf("write desktop entry: %w", err)
	}
	if output, err := exec.Command("xdg-mime", "default", desktopFilename, schemeMIMEType).CombinedOutput(); err != nil {
		return fmt.Errorf("register URL scheme with xdg-mime: %w: %s", err, strings.TrimSpace(string(output)))
	}
	updateDesktopDatabase(filepath.Dir(r.desktopFile))
	return nil
}

func (r *linuxRegistrar) Status() (Status, error) {
	_, binaryErr := os.Stat(r.executable)
	_, desktopErr := os.Stat(r.desktopFile)
	installed := binaryErr == nil && desktopErr == nil
	if _, err := exec.LookPath("xdg-mime"); err != nil {
		return Status{Installed: installed, Detail: "xdg-mime unavailable"}, nil
	}
	output, err := exec.Command("xdg-mime", "query", "default", schemeMIMEType).CombinedOutput()
	if err != nil {
		return Status{}, fmt.Errorf("query URL scheme: %w", err)
	}
	defaultHandler := strings.TrimSpace(string(output))
	return Status{Installed: installed, Active: installed && defaultHandler == desktopFilename, Detail: defaultHandler}, nil
}

func (r *linuxRegistrar) Uninstall() error {
	for _, mimeapps := range []string{
		filepath.Join(r.configHome, "mimeapps.list"),
		filepath.Join(r.dataHome, "applications", "mimeapps.list"),
	} {
		if err := removeMIMEAssociation(mimeapps); err != nil {
			return err
		}
	}
	if err := os.Remove(r.desktopFile); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove desktop entry: %w", err)
	}
	if err := os.Remove(r.executable); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove handler binary: %w", err)
	}
	updateDesktopDatabase(filepath.Dir(r.desktopFile))
	return RemoveHandlerDiagnostic()
}

func desktopQuote(value string) string {
	replacer := strings.NewReplacer(`\`, `\\`, `"`, `\"`, "`", `\`+"`", "$", `\$`)
	return `"` + replacer.Replace(value) + `"`
}

func updateDesktopDatabase(directory string) {
	if executable, err := exec.LookPath("update-desktop-database"); err == nil {
		_ = exec.Command(executable, directory).Run()
	}
}

func removeMIMEAssociation(filename string) error {
	content, err := os.ReadFile(filename)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("read %s: %w", filename, err)
	}
	info, err := os.Stat(filename)
	if err != nil {
		return fmt.Errorf("stat %s: %w", filename, err)
	}

	var output bytes.Buffer
	scanner := bufio.NewScanner(bytes.NewReader(content))
	for scanner.Scan() {
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, schemeMIMEType+"=") {
			parts := strings.Split(strings.TrimPrefix(trimmed, schemeMIMEType+"="), ";")
			kept := parts[:0]
			for _, part := range parts {
				if part != "" && part != desktopFilename {
					kept = append(kept, part)
				}
			}
			if len(kept) == 0 {
				continue
			}
			line = schemeMIMEType + "=" + strings.Join(kept, ";") + ";"
		}
		output.WriteString(line)
		output.WriteByte('\n')
	}
	if err := scanner.Err(); err != nil {
		return fmt.Errorf("parse %s: %w", filename, err)
	}
	if err := writeFileAtomic(filename, output.Bytes(), info.Mode().Perm()); err != nil {
		return fmt.Errorf("update %s: %w", filename, err)
	}
	return nil
}

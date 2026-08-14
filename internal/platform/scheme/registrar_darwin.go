//go:build darwin

package scheme

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

const (
	macBundleIdentifier = "com.github.tukiminya.omnooth-url-handler"
	customURLScheme     = "booth-library-manager"
)

type darwinRegistrar struct {
	executable string
	appBundle  string
}

func NewRegistrar() (Registrar, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, fmt.Errorf("find home directory: %w", err)
	}
	return &darwinRegistrar{
		executable: filepath.Join(home, "Library", "Application Support", "omnooth", "bin", "omnooth"),
		appBundle:  filepath.Join(home, "Applications", "Omnooth URL Handler.app"),
	}, nil
}

func (r *darwinRegistrar) Install() error {
	if _, err := exec.LookPath("osacompile"); err != nil {
		return fmt.Errorf("osacompile is required to register the URL scheme: %w", err)
	}
	if err := installExecutable(r.executable); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(r.appBundle), 0o755); err != nil {
		return fmt.Errorf("create Applications directory: %w", err)
	}
	temporary, err := os.CreateTemp("", "omnooth-handler-*.applescript")
	if err != nil {
		return fmt.Errorf("create AppleScript source: %w", err)
	}
	temporaryName := temporary.Name()
	defer os.Remove(temporaryName)
	script := handlerAppleScript(r.executable)
	if _, err := temporary.WriteString(script); err != nil {
		temporary.Close()
		return fmt.Errorf("write AppleScript source: %w", err)
	}
	if err := temporary.Close(); err != nil {
		return fmt.Errorf("close AppleScript source: %w", err)
	}
	if err := os.RemoveAll(r.appBundle); err != nil {
		return fmt.Errorf("replace URL handler app: %w", err)
	}
	if output, err := exec.Command("osacompile", "-o", r.appBundle, temporaryName).CombinedOutput(); err != nil {
		return fmt.Errorf("compile URL handler app: %w: %s", err, strings.TrimSpace(string(output)))
	}

	plist := filepath.Join(r.appBundle, "Contents", "Info.plist")
	if err := plutilSet(plist, "CFBundleIdentifier", "-string", macBundleIdentifier); err != nil {
		return err
	}
	if err := plutilSet(plist, "LSUIElement", "-bool", "YES"); err != nil {
		return err
	}
	urlTypes := `[{"CFBundleTypeRole":"Viewer","CFBundleURLName":"com.github.tukiminya.omnooth.booth-library-manager","CFBundleURLSchemes":["booth-library-manager"]}]`
	if err := plutilSet(plist, "CFBundleURLTypes", "-json", urlTypes); err != nil {
		return err
	}
	if output, err := exec.Command("codesign", "--force", "--deep", "--sign", "-", r.appBundle).CombinedOutput(); err != nil {
		return fmt.Errorf("ad-hoc sign URL handler app: %w: %s", err, strings.TrimSpace(string(output)))
	}
	registerTool := "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
	if _, err := os.Stat(registerTool); err == nil {
		if output, err := exec.Command(registerTool, "-f", r.appBundle).CombinedOutput(); err != nil {
			return fmt.Errorf("register URL handler app: %w: %s", err, strings.TrimSpace(string(output)))
		}
	} else if output, err := exec.Command("open", "-gj", r.appBundle).CombinedOutput(); err != nil {
		return fmt.Errorf("register URL handler app: %w: %s", err, strings.TrimSpace(string(output)))
	}
	if err := setDefaultApplication(r.appBundle); err != nil {
		return err
	}
	// アプリバンドルの登録だけでは、その機能をOSへ通知したにすぎない。
	// Launch ServicesがCustom URLのハンドラーとしてこのアプリを実際に解決できた時点で、
	// インストール完了とする。lsregisterの登録一覧だけを見ると偽陽性になり得る。
	return waitForDefaultApplication(r.appBundle, 3*time.Second)
}

func (r *darwinRegistrar) Status() (Status, error) {
	_, binaryErr := os.Stat(r.executable)
	_, appErr := os.Stat(filepath.Join(r.appBundle, "Contents", "Info.plist"))
	installed := binaryErr == nil && appErr == nil
	detail := "not installed"
	if !installed {
		return Status{Detail: detail}, nil
	}
	detail = r.appBundle
	defaultApplication, err := defaultApplicationForScheme()
	if err != nil {
		return Status{Installed: true, Detail: detail + " (default handler query failed)"}, nil
	}
	active := samePath(defaultApplication, r.appBundle)
	if !active {
		if defaultApplication == "" {
			detail += " (default handler: none)"
		} else {
			detail += " (default handler: " + defaultApplication + ")"
		}
	}
	return Status{Installed: true, Active: active, Detail: detail}, nil
}

func (r *darwinRegistrar) Uninstall() error {
	registerTool := "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
	if _, err := os.Stat(r.appBundle); err == nil {
		if output, unregisterErr := exec.Command(registerTool, "-u", r.appBundle).CombinedOutput(); unregisterErr != nil {
			return fmt.Errorf("unregister URL handler app: %w: %s", unregisterErr, strings.TrimSpace(string(output)))
		}
	}
	if err := os.RemoveAll(r.appBundle); err != nil {
		return fmt.Errorf("remove URL handler app: %w", err)
	}
	if err := os.Remove(r.executable); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove handler binary: %w", err)
	}
	return RemoveHandlerDiagnostic()
}

func handlerAppleScript(executable string) string {
	// 取込に時間がかかってもApple Eventハンドラーはすぐに応答する必要がある。
	// nohupと標準ストリームの切断でCLIを短命なアプレットから分離し、シェルの引用処理で
	// 受信URL全体を一つの引数として渡す。
	return fmt.Sprintf("on open location incomingURL\n\tdo shell script \"/usr/bin/nohup \" & quoted form of \"%s\" & \" scheme handle \" & quoted form of incomingURL & \" >/dev/null 2>&1 &\"\nend open location\n", appleScriptString(executable))
}

func setDefaultApplication(appBundle string) error {
	// AppleScriptObjCを使うことで、利用者の環境にSwiftやXcode Command Line Toolsを
	// 要求せず、インストール済みのGoバイナリからNSWorkspaceを利用できる。
	arguments := []string{
		"-e", `use framework "Foundation"`,
		"-e", `use framework "AppKit"`,
		"-e", `on run argv`,
		"-e", `set appURL to current application's NSURL's fileURLWithPath:(item 1 of argv)`,
		"-e", `set schemeName to item 2 of argv`,
		"-e", `current application's NSWorkspace's sharedWorkspace()'s setDefaultApplicationAtURL:appURL toOpenURLsWithScheme:schemeName completionHandler:(missing value)`,
		"-e", `end run`,
		appBundle,
		customURLScheme,
	}
	if output, err := exec.Command("/usr/bin/osascript", arguments...).CombinedOutput(); err != nil {
		return fmt.Errorf("set default URL handler: %w: %s", err, strings.TrimSpace(string(output)))
	}
	return nil
}

func defaultApplicationForScheme() (string, error) {
	// 登録済みバンドルが有効なハンドラーとは限らないため、登録一覧ではなく、
	// URLに対してOSが実際に解決するアプリを問い合わせる。
	arguments := []string{
		"-e", `use framework "Foundation"`,
		"-e", `use framework "AppKit"`,
		"-e", fmt.Sprintf(`set targetURL to current application's NSURL's URLWithString:"%s://item-import"`, customURLScheme),
		"-e", `set appURL to current application's NSWorkspace's sharedWorkspace()'s URLForApplicationToOpenURL:targetURL`,
		"-e", `if appURL is missing value then return ""`,
		"-e", `return appURL's |path|() as text`,
	}
	output, err := exec.Command("/usr/bin/osascript", arguments...).CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("query default URL handler: %w: %s", err, strings.TrimSpace(string(output)))
	}
	return strings.TrimSpace(string(output)), nil
}

func waitForDefaultApplication(appBundle string, timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	var current string
	for {
		var err error
		current, err = defaultApplicationForScheme()
		if err != nil {
			return err
		}
		if samePath(current, appBundle) {
			return nil
		}
		if time.Now().After(deadline) {
			if current == "" {
				current = "none"
			}
			return fmt.Errorf("register URL handler: macOS reports %s as the default handler", current)
		}
		time.Sleep(100 * time.Millisecond)
	}
}

func plutilSet(plist, key, valueType, value string) error {
	replaceArgs := []string{"-replace", key, valueType, value, plist}
	if output, err := exec.Command("plutil", replaceArgs...).CombinedOutput(); err == nil {
		return nil
	} else {
		insertArgs := []string{"-insert", key, valueType, value, plist}
		if insertOutput, insertErr := exec.Command("plutil", insertArgs...).CombinedOutput(); insertErr != nil {
			return fmt.Errorf("update URL handler Info.plist: %w: %s; %s", insertErr, strings.TrimSpace(string(output)), strings.TrimSpace(string(insertOutput)))
		}
		return nil
	}
}

func appleScriptString(value string) string {
	return strings.NewReplacer(`\`, `\\`, `"`, `\"`).Replace(value)
}

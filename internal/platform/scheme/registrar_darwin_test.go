//go:build darwin

package scheme

import (
	"strings"
	"testing"
)

func TestHandlerAppleScriptStartsSanitizedBackgroundCommand(t *testing.T) {
	script := handlerAppleScript(`/Applications/Test "Handler"/omnooth`)
	for _, expected := range []string{
		`/usr/bin/nohup`,
		`scheme handle`,
		`quoted form of incomingURL`,
		`>/dev/null 2>&1 &`,
	} {
		if !strings.Contains(script, expected) {
			t.Errorf("handler script does not contain %q:\n%s", expected, script)
		}
	}
	if strings.Contains(script, ` import `) {
		t.Fatalf("handler script still invokes the synchronous import command:\n%s", script)
	}
}

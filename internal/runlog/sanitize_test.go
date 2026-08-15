package runlog

import (
	"strings"
	"testing"
)

func TestSanitizeRedactsDetachedSecretParameters(t *testing.T) {
	value := "signature=one token=two access_token=three X-Amz-Credential=four"
	clean := Sanitize(value)
	for _, secret := range []string{"one", "two", "three", "four"} {
		if strings.Contains(clean, secret) {
			t.Fatalf("sanitized value contains %q: %s", secret, clean)
		}
	}
}

package importer

import (
	"errors"
	"net/url"
	"strings"
)

var ErrUntrustedDownloadURL = errors.New("download URL is not an approved BOOTH HTTPS URL")

// ValidateDownloadURL は、初回リクエストと各リダイレクト先に同じ許可リストを適用する。
func ValidateDownloadURL(u *url.URL) error {
	if u == nil || u.Opaque != "" || !strings.EqualFold(u.Scheme, "https") ||
		u.User != nil || u.Fragment != "" || (u.Port() != "" && u.Port() != "443") {
		return ErrUntrustedDownloadURL
	}

	host := strings.ToLower(u.Hostname())
	if host == "" || (host != "booth.pm" && !strings.HasSuffix(host, ".booth.pm")) {
		return ErrUntrustedDownloadURL
	}
	return nil
}

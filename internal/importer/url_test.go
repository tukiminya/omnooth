package importer

import (
	"net/url"
	"testing"
)

func TestValidateDownloadURL(t *testing.T) {
	tests := []struct {
		name    string
		raw     string
		wantErr bool
	}{
		{"root", "https://booth.pm/file", false},
		{"subdomain", "https://download.booth.pm/file", false},
		{"explicit HTTPS port", "https://booth.pm:443/file", false},
		{"HTTP", "http://booth.pm/file", true},
		{"suffix attack", "https://booth.pm.evil.example/file", true},
		{"prefix attack", "https://evilbooth.pm/file", true},
		{"userinfo", "https://booth.pm@evil.example/file", true},
		{"nonstandard port", "https://booth.pm:8443/file", true},
		{"fragment", "https://booth.pm/file#secret", true},
		{"trailing dot", "https://booth.pm./file", true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			u, err := url.Parse(test.raw)
			if err != nil {
				t.Fatal(err)
			}
			err = ValidateDownloadURL(u)
			if (err != nil) != test.wantErr {
				t.Fatalf("ValidateDownloadURL() error = %v, wantErr %v", err, test.wantErr)
			}
		})
	}
}

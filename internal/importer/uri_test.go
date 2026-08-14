package importer

import (
	"errors"
	"net/url"
	"strings"
	"testing"
)

func validImportURI() string {
	query := url.Values{
		"dlurl":                 {"https://download.booth.pm/file?X-Amz-Signature=secret"},
		"downloadable_filename": {"衣装 v1.0.zip"},
		"item_id":               {"8657397"},
		"order_id":              {"123"},
		"variation_id":          {"14460999"},
		"future_parameter":      {"ignored"},
	}
	return "booth-library-manager://item-import?" + query.Encode()
}

func TestParseImportURI(t *testing.T) {
	request, err := ParseImportURI(validImportURI())
	if err != nil {
		t.Fatalf("ParseImportURI() error = %v", err)
	}
	if request.ItemID != 8657397 || request.OrderID != 123 || request.VariationID != 14460999 {
		t.Fatalf("unexpected IDs: %+v", request)
	}
	if request.DownloadableFilename != "衣装 v1.0.zip" {
		t.Fatalf("filename = %q", request.DownloadableFilename)
	}
	if request.DownloadURL.Hostname() != "download.booth.pm" {
		t.Fatalf("download host = %q", request.DownloadURL.Hostname())
	}
	if request.DownloadURL.Query().Get("X-Amz-Signature") != "secret" {
		t.Fatalf("encoded dlurl was not decoded correctly: %q", request.DownloadURL.String())
	}
}

func TestParseImportURIAcceptsGiftWithoutOrderID(t *testing.T) {
	tests := map[string]string{
		"missing": strings.Replace(validImportURI(), "&order_id=123", "", 1),
		"empty":   strings.Replace(validImportURI(), "order_id=123", "order_id", 1),
	}
	for name, raw := range tests {
		t.Run(name, func(t *testing.T) {
			request, err := ParseImportURI(raw)
			if err != nil {
				t.Fatalf("ParseImportURI() error = %v", err)
			}
			if request.OrderID != 0 {
				t.Fatalf("OrderID = %d, want 0", request.OrderID)
			}
		})
	}
}

func TestParseImportURIRejectsMalformedInputWithoutLeakingURL(t *testing.T) {
	tests := map[string]string{
		"wrong scheme":    strings.Replace(validImportURI(), "booth-library-manager", "evil", 1),
		"wrong action":    strings.Replace(validImportURI(), "item-import", "delete", 1),
		"missing value":   strings.Replace(validImportURI(), "item_id=8657397&", "", 1),
		"duplicate value": validImportURI() + "&item_id=1",
		"invalid id":      strings.Replace(validImportURI(), "order_id=123", "order_id=0", 1),
		"duplicate order": validImportURI() + "&order_id=456",
		"untrusted host":  strings.Replace(validImportURI(), "download.booth.pm", "booth.pm.evil.example", 1),
	}
	for name, raw := range tests {
		t.Run(name, func(t *testing.T) {
			_, err := ParseImportURI(raw)
			if err == nil {
				t.Fatal("expected an error")
			}
			if strings.Contains(err.Error(), "secret") {
				t.Fatalf("error leaked signed URL: %v", err)
			}
			if !errors.Is(err, ErrInvalidImportURI) && !errors.Is(err, ErrUntrustedDownloadURL) {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

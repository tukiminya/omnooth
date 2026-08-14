package cmd

import (
	"context"
	"errors"
	"fmt"
	"testing"

	"github.com/tukiminya/omnooth/internal/importer"
)

func TestHandlerFailureCodeDoesNotExposeErrorDetails(t *testing.T) {
	tests := []struct {
		err  error
		want string
	}{
		{fmt.Errorf("parse: %w", importer.ErrInvalidImportURI), "invalid_import_uri"},
		{fmt.Errorf("validate: %w", importer.ErrUntrustedDownloadURL), "untrusted_download_url"},
		{context.Canceled, "canceled"},
		{context.DeadlineExceeded, "canceled"},
		{errors.New("request https://booth.pm/?signature=secret failed"), "import_failed"},
	}
	for _, test := range tests {
		if got := handlerFailureCode(test.err); got != test.want {
			t.Errorf("handlerFailureCode(%v) = %q, want %q", test.err, got, test.want)
		}
	}
}

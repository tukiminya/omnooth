package cmd

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/tukiminya/omnooth/internal/importer"
	"github.com/tukiminya/omnooth/internal/outdated"
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

func TestWriteOutdated(t *testing.T) {
	var output strings.Builder
	err := writeOutdated(&output, outdated.Result{Reports: []outdated.Report{{
		Status: outdated.StatusOutdated, ItemID: 1, ItemName: "Item",
		VariationID: 3, VariationName: "Standard",
		LocalDownloadables: []string{"old.zip"}, Changes: []string{"added: new.zip"},
	}}})
	if err != nil {
		t.Fatal(err)
	}
	for _, expected := range []string{"STATUS", "outdated", "1 Item", "3 Standard", "old.zip", "added: new.zip"} {
		if !strings.Contains(output.String(), expected) {
			t.Fatalf("output = %q, missing %q", output.String(), expected)
		}
	}

	output.Reset()
	if err := writeOutdated(&output, outdated.Result{}); err != nil {
		t.Fatal(err)
	}
	if output.String() != "All installed items are up to date.\n" {
		t.Fatalf("output = %q", output.String())
	}
}

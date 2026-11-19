package fetch

import (
	"bytes"
	"crypto/sha256"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// BenchmarkParseChecksumText：校验文本解析的纯 CPU 基准。
func BenchmarkParseChecksumText(b *testing.B) {
	line := strings.Repeat("a1", 32) + "  mox-proot-linux-amd64-musl\n"
	b.SetBytes(int64(len(line)))
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := ParseChecksumText(line); err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkDownloadLocal：本机环回 HTTP 服务端到端下载基准（含流式 SHA-256）。
// 数字反映的是本机回环 + 内存/磁盘带宽，不代表公网；仅用于回归性能量级。
func BenchmarkDownloadLocal(b *testing.B) {
	const size = 4 << 20 // 4 MiB
	payload := bytes.Repeat([]byte("mox-proot-benchmark-"), size/20+1)[:size]
	sum := fmt.Sprintf("%x", sha256.Sum256(payload))
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, ".sha256") {
			fmt.Fprintln(w, sum)
			return
		}
		_, _ = w.Write(payload)
	}))
	defer srv.Close()

	dir := b.TempDir()
	b.SetBytes(int64(size))
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		dest := filepath.Join(dir, fmt.Sprintf("asset-%d", i))
		err := Download(Options{
			Channels: []string{srv.URL},
			Asset:    "asset",
			Dest:     dest,
			Retries:  1,
		})
		if err != nil {
			b.Fatal(err)
		}
		_ = os.Remove(dest)
	}
}

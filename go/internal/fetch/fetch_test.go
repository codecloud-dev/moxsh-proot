package fetch

import (
	"bytes"
	"crypto/sha256"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestParseChecksumText(t *testing.T) {
	valid := "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
	cases := []struct {
		in      string
		want    string
		wantErr bool
	}{
		{valid, valid, false},
		{valid + "  mox-proot-linux-amd64", valid, false}, // sha256sum 格式
		{"  " + valid + "\n", valid, false},               // 空白与换行
		{"A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4E5F6A1B2", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2", false},
		{"", "", true},
		{"short", "", true},
		{valid[:63], "", true},
		{valid[:63] + "g", "", true}, // 非 hex 字符
	}
	for _, c := range cases {
		got, err := ParseChecksumText(c.in)
		if c.wantErr {
			if err == nil {
				t.Errorf("ParseChecksumText(%q) 应报错", c.in)
			}
			continue
		}
		if err != nil {
			t.Errorf("ParseChecksumText(%q) 意外错误: %v", c.in, err)
			continue
		}
		if got != c.want {
			t.Errorf("ParseChecksumText(%q) = %q, want %q", c.in, got, c.want)
		}
	}
}

func TestURLBuilders(t *testing.T) {
	if got := AssetURL("https://example.com/base/", "asset"); got != "https://example.com/base/asset" {
		t.Errorf("AssetURL = %q", got)
	}
	if got := ChecksumURL("https://example.com/base", "asset"); got != "https://example.com/base/asset.sha256" {
		t.Errorf("ChecksumURL = %q", got)
	}
}

func TestDownloadValidatesArgs(t *testing.T) {
	if err := Download(Options{}); err == nil {
		t.Error("空选项应报错")
	}
	if err := Download(Options{Channels: []string{"https://x"}}); err == nil {
		t.Error("缺资产名/目标应报错")
	}
}

// TestErrIdentity 错误哨兵可被 errors.Is 识别（调用方据此区分"全失败"与"校验失败"）。
func TestErrIdentity(t *testing.T) {
	if !errors.Is(ErrChecksumMismatch, ErrChecksumMismatch) {
		t.Fatal("ErrChecksumMismatch 不可识别")
	}
	if !errors.Is(ErrAllChannelsFailed, ErrAllChannelsFailed) {
		t.Fatal("ErrAllChannelsFailed 不可识别")
	}
}

// ---- 回归测试：断点续传 / 校验 fail-closed（2026-11-19 深度升级） ----

// TestDownloadRecoversFrom416：当 .part 已经是完整文件（或同长度的陈旧文件）时，
// 服务器对 Range 请求回 416。下载器必须能从 416 恢复（去掉 Range 重新完整下载），
// 而不是把 416 当成永久失败导致所有通道失败。
func TestDownloadRecoversFrom416(t *testing.T) {
	body := []byte("mox-proot complete binary payload v0.9.2")
	sum := fmt.Sprintf("%x", sha256.Sum256(body))
	var rangeReq, fullReq int
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, ".sha256") {
			fmt.Fprintln(w, sum)
			return
		}
		if r.Header.Get("Range") != "" {
			rangeReq++
			w.WriteHeader(http.StatusRequestedRangeNotSatisfiable) // 416
			return
		}
		fullReq++
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(body)
	}))
	defer srv.Close()

	dest := filepath.Join(t.TempDir(), "asset")
	// 预先落一个与完整文件同样大小的陈旧 .part（触发 Range → 416）
	if err := os.WriteFile(dest, bytes.Repeat([]byte("X"), len(body)), 0o755); err != nil {
		t.Fatal(err)
	}
	err := Download(Options{
		Channels:       []string{srv.URL},
		Asset:          "asset",
		Dest:           dest,
		ExpectedSHA256: sum,
		Retries:        1,
	})
	if err != nil {
		t.Fatalf("416 场景应能恢复并下载成功，实际失败: %v", err)
	}
	got, _ := os.ReadFile(dest)
	if !bytes.Equal(got, body) {
		t.Fatalf("恢复后内容不符: got %q want %q", got, body)
	}
	if rangeReq == 0 || fullReq == 0 {
		t.Fatalf("应至少发过一次 Range(%d) 并回退完整 GET(%d)", rangeReq, fullReq)
	}
}

// TestDownloadFailsClosedWithoutChecksum：主通道拿不到 .sha256（404）时，
// 下载器不能放行"零校验"的二进制——镜像被污染会静默装进系统。
// 正确行为：所有拿不到校验值的通道一律跳过，返回 ErrAllChannelsFailed，
// 且不把未校验内容留在目标路径。
func TestDownloadFailsClosedWithoutChecksum(t *testing.T) {
	corrupted := []byte("CORRUPTED-MIRROR-PAYLOAD")
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, ".sha256") {
			http.NotFound(w, r) // 校验文件缺失
			return
		}
		_, _ = w.Write(corrupted)
	}))
	defer srv.Close()

	dest := filepath.Join(t.TempDir(), "asset")
	err := Download(Options{
		Channels: []string{srv.URL},
		Asset:    "asset",
		Dest:     dest,
		Retries:  1,
	})
	if err == nil {
		t.Fatal("无校验值时不应静默接受下载内容（fail-open 漏洞）")
	}
	if !errors.Is(err, ErrAllChannelsFailed) {
		t.Fatalf("期望 ErrAllChannelsFailed，实际: %v", err)
	}
	if got, rerr := os.ReadFile(dest); rerr == nil && bytes.Equal(got, corrupted) {
		t.Fatal("未校验的污染内容不应留在目标路径")
	}
}

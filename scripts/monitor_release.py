#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
GitHub Actions Workflow Progress Monitor
Real-time progress tracker for Nyan-setup release workflows.
"""

import argparse
import io
import json
import os
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime

# Ensure proper UTF-8 output under Windows console
if sys.platform == "win32":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")

DEFAULT_REPO = "ayaya-any/Nyan-setup"

def get_headers():
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "Nyan-Setup-Release-Monitor/1.0",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers

def api_get(url):
    req = urllib.request.Request(url, headers=get_headers())
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            data = resp.read().decode("utf-8")
            rate_remaining = resp.headers.get("x-ratelimit-remaining")
            return json.loads(data), rate_remaining
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None, None
        error_body = e.read().decode("utf-8", errors="ignore")
        print(f"[!] HTTP {e.code} Error for {url}: {error_body}", file=sys.stderr)
        return None, None
    except Exception as e:
        print(f"[!] Request error: {e}", file=sys.stderr)
        return None, None

def find_workflow_run(repo, tag, max_wait_sec=120, poll_sec=5):
    url = f"https://api.github.com/repos/{repo}/actions/runs?event=push&per_page=15"
    print(f"[*] 正在检索仓库 {repo} 中与标签 [{tag}] 关联的 GitHub Actions 工作流...")
    start_time = time.time()
    dots = 0

    while time.time() - start_time < max_wait_sec:
        data, remaining = api_get(url)
        if data and "workflow_runs" in data:
            for run in data["workflow_runs"]:
                branch = run.get("head_branch") or ""
                # Match latest run for this tag
                if branch == tag or tag in run.get("display_title", ""):
                    print(f"\n[+] 找到工作流运行: Run ID {run['id']} ({run['name']})")
                    print(f"    网页链接: {run.get('html_url')}")
                    return run

        dots = (dots + 1) % 4
        elapsed = int(time.time() - start_time)
        print(f"\r    等待工作流被 GitHub 调度触发中... ({elapsed}s){'.' * dots:<4}", end="", flush=True)
        time.sleep(poll_sec)

    print(f"\n[!] 超时: 在 {max_wait_sec} 秒内未检索到 tag [{tag}] 触发的运行实例。")
    return None

def monitor_run(repo, run_id, poll_sec=8):
    run_url = f"https://api.github.com/repos/{repo}/actions/runs/{run_id}"
    jobs_url = f"https://api.github.com/repos/{repo}/actions/runs/{run_id}/jobs"

    start_monitor = time.time()

    while True:
        run_data, _ = api_get(run_url)
        if not run_data:
            time.sleep(poll_sec)
            continue

        status = run_data.get("status")  # queued, in_progress, completed
        conclusion = run_data.get("conclusion")  # success, failure, cancelled, etc.
        elapsed = int(time.time() - start_monitor)

        jobs_data, _ = api_get(jobs_url)
        job_lines = []
        if jobs_data and "jobs" in jobs_data:
            for job in jobs_data["jobs"]:
                j_name = job.get("name", "Job")
                j_status = job.get("status", "")
                j_conclusion = job.get("conclusion") or ""

                job_header = f"  Job: {j_name} [{j_status}{f' -> {j_conclusion}' if j_conclusion else ''}]"
                job_lines.append(job_header)

                steps = job.get("steps", [])
                for step in steps:
                    s_name = step.get("name", "")
                    s_status = step.get("status", "")
                    s_conc = step.get("conclusion")

                    if s_conc == "success":
                        icon = "[OK]"
                    elif s_conc in ("failure", "timed_out"):
                        icon = "[ERR]"
                    elif s_conc == "skipped":
                        icon = "[SKIP]"
                    elif s_status == "in_progress":
                        icon = "[RUNNING]"
                    elif s_status == "queued":
                        icon = "[QUEUED]"
                    else:
                        icon = "[WAIT]"

                    job_lines.append(f"    {icon:<10} {s_name}")

        now_str = datetime.now().strftime("%H:%M:%S")

        print("=" * 64)
        print(f"[{now_str}] 状态: {status.upper()} | 结论: {str(conclusion).upper()} | 监控时长: {elapsed}s")
        print(f"URL: {run_data.get('html_url')}")
        if job_lines:
            print("\n".join(job_lines))
        print("=" * 64)

        if status == "completed":
            print(f"\n[*] 工作流执行结束，最终结论: {conclusion}")
            return conclusion == "success"

        time.sleep(poll_sec)

def check_release_assets(repo, tag):
    release_url = f"https://api.github.com/repos/{repo}/releases/tags/{tag}"
    print(f"\n[*] 查询 GitHub Release 详情与上传资产 ({tag})...")
    # Poll a few times if release is still finalizing
    for _ in range(5):
        data, _ = api_get(release_url)
        if data and data.get("assets"):
            break
        time.sleep(3)

    if not data:
        print("[!] 暂未获取到 Release 信息，可能正在同步中。")
        return

    print(f"\n[+] GitHub Release 发布成功！")
    print(f"    标题: {data.get('name') or data.get('tag_name')}")
    print(f"    页面: {data.get('html_url')}")
    assets = data.get("assets", [])
    if assets:
        print(f"\n    已上传附件列表 ({len(assets)} 个):")
        for asset in assets:
            sz_mb = asset.get("size", 0) / (1024 * 1024)
            print(f"      - {asset.get('name')} ({sz_mb:.2f} MB)")
            print(f"        下载: {asset.get('browser_download_url')}")
    else:
        print("    (暂无附件)")

def main():
    parser = argparse.ArgumentParser(description="GitHub Actions Release Progress Monitor")
    parser.add_argument("--repo", default=DEFAULT_REPO, help="GitHub repository (owner/repo)")
    parser.add_argument("--tag", default="v0.1.0", help="Git Tag to monitor")
    parser.add_argument("--interval", type=int, default=8, help="Polling interval in seconds")
    args = parser.parse_args()

    run = find_workflow_run(args.repo, args.tag)
    if not run:
        sys.exit(1)

    success = monitor_run(args.repo, run["id"], poll_sec=args.interval)
    if success:
        check_release_assets(args.repo, args.tag)
    else:
        print(f"\n[!] 工作流未成功完成，请访问上面的运行链接排查日志。")
        sys.exit(2)

if __name__ == "__main__":
    main()

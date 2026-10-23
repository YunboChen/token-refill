// Windows 一律使用 GUI 子系统：若 debug 构建保留控制台子系统，从资源管理器
// 双击会额外拉起一个控制台窗口（默认由 Windows Terminal 承载）。
// 从终端启动（cargo run / tauri dev）时 stdio 仍被继承，日志和 panic 照常输出。
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    token_refill_lib::run();
}

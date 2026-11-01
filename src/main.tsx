import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { domAnimation, LazyMotion, MotionConfig } from 'motion/react';
import App from './App';
import TrayPanel from './panels/TrayPanel';
import FloatingBall from './panels/FloatingBall';
import FloatingBallPopover from './panels/FloatingBallPopover';
import { AppStateProvider } from './lib/store';
import { RelayProvider } from './lib/relay';
import { loadTheme, saveTheme } from './lib/prefs';
import { applyTheme, useThemeSync } from './lib/theme';
import { WINDOW_VIEW } from './lib/view';
import './styles.css';

// 首帧前落定外观，避免深色模式下先闪一帧浅色。
// 旧版本的草稿键只在首次启动时读取，主题同时迁移到独立键。
const initialTheme = loadTheme();
saveTheme(initialTheme);
applyTheme(initialTheme);

// 菜单栏面板与悬浮球都是透明窗口；主窗口保持默认画布背景。
if (WINDOW_VIEW !== 'main') {
  document.documentElement.dataset.view = WINDOW_VIEW;
}

function renderView() {
  switch (WINDOW_VIEW) {
    case 'tray':
      return <TrayPanel />;
    case 'float':
      return <FloatingBall />;
    case 'float-popover':
      return <FloatingBallPopover />;
    default:
      return <App />;
  }
}

function Root() {
  // 主窗口切换外观后，菜单栏面板、悬浮球与弹层都要跟着换色。
  useThemeSync();
  return (
    <StrictMode>
      <LazyMotion features={domAnimation} strict>
        <MotionConfig
          reducedMotion="user"
          transition={{ type: 'spring', stiffness: 380, damping: 30 }}
        >
          <AppStateProvider>
            <RelayProvider>{renderView()}</RelayProvider>
          </AppStateProvider>
        </MotionConfig>
      </LazyMotion>
    </StrictMode>
  );
}

createRoot(document.getElementById('root')!).render(<Root />);

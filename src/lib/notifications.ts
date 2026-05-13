import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification';

export const RESTART_COMMAND = 'pkill -f "claude " && exec $SHELL -l';

export async function ensureNotificationPermission(): Promise<boolean> {
  let granted = await isPermissionGranted();
  if (!granted) {
    const result = await requestPermission();
    granted = result === 'granted';
  }
  return granted;
}

export async function notifyActivation(profileName: string): Promise<void> {
  if (!(await ensureNotificationPermission())) return;
  sendNotification({
    title: `Activated: ${profileName}`,
    body: 'Restart Claude Code to apply the new env.',
  });
}

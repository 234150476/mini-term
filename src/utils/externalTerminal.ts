import { invoke } from '@tauri-apps/api/core';

export async function activateProjectTerminal(
  projectId: string,
  projectPath: string,
  projectName: string,
): Promise<void> {
  await invoke('activate_project_terminal', {
    projectId,
    projectPath,
    projectName,
  });
}

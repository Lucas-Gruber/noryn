// Not a valid team ID, so a team named "all" remains independently selectable.
export const ALL_WORKSPACES = '*';
export function includesWorkspace(selection: string, workspace: string) {
  return selection === ALL_WORKSPACES || selection === workspace;
}

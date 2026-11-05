// The folder of the export: choosing it, the folders used before and the permission to read them. The code is the kit's
// (appkit-web/folder); this names what is this page's own. Every local file shares the origin file://, so the handles are
// kept in a database of this page's name and no other is ever opened: another page's folders stay out of reach of this one.

import {
  grantAccess as kitGrantAccess,
  permissionOf as kitPermissionOf,
  pickFolder as kitPickFolder,
  type AccessMode,
  type DirHandle,
  type PermissionState,
  type PickerWindow,
  type PickResult,
} from 'appkit-web/folder';

/** The page only reads. */
export const MODE: AccessMode = 'read';
/** The picker remembers the last folder per id and starts there. */
export const PICKER_ID = 'testing-web-export';
/** The database of the folders used before. */
export const HANDLES_DB = 'testing-web';

export function pickFolder(win: PickerWindow): Promise<PickResult> {
  return kitPickFolder(win, { id: PICKER_ID, mode: MODE });
}

export function permissionOf(handle: DirHandle): Promise<PermissionState | 'gone'> {
  return kitPermissionOf(handle, MODE);
}

export function grantAccess(handle: DirHandle): Promise<PermissionState | 'gone'> {
  return kitGrantAccess(handle, MODE);
}

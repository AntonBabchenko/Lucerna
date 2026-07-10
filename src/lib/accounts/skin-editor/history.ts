// Snapshot-based undo/redo for the skin buffer. One stroke = one entry:
// call begin(current) at the START of every stroke (pointerdown), then paint.
// 64x64x4 = 16 KB per snapshot; a cap of 50 keeps worst case under 1 MB.

export class SkinHistory {
  private undoStack: Uint8ClampedArray[] = [];
  private redoStack: Uint8ClampedArray[] = [];

  constructor(private readonly cap = 50) {}

  get canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  get canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  get depth(): number {
    return this.undoStack.length;
  }

  begin(current: Uint8ClampedArray): void {
    this.undoStack.push(new Uint8ClampedArray(current));
    if (this.undoStack.length > this.cap) this.undoStack.shift();
    this.redoStack = [];
  }

  undo(current: Uint8ClampedArray): Uint8ClampedArray | null {
    const prev = this.undoStack.pop();
    if (!prev) return null;
    this.redoStack.push(new Uint8ClampedArray(current));
    return prev;
  }

  redo(current: Uint8ClampedArray): Uint8ClampedArray | null {
    const next = this.redoStack.pop();
    if (!next) return null;
    this.undoStack.push(new Uint8ClampedArray(current));
    return next;
  }
}

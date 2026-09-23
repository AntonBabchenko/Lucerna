// One fact: whether the close dialog is on screen. Read by
// `screenOwnedElsewhere()`, so a contextual tour yields rather than painting
// its scrim over the question and taking its Escape.
export const closeAskState = $state({ open: false });

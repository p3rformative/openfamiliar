export const UPDATER_UNAVAILABLE_MESSAGE =
  "Las actualizaciones firmadas no están habilitadas.";

export const UPDATER_UP_TO_DATE_MESSAGE =
  "No se encontró una actualización firmada.";

const EMPTY_ENDPOINTS_ERROR = "Updater does not have any endpoints set.";

export function mapUpdaterError(message: string): string {
  const text = message.trim();
  if (text === EMPTY_ENDPOINTS_ERROR || text.endsWith(EMPTY_ENDPOINTS_ERROR)) {
    return UPDATER_UNAVAILABLE_MESSAGE;
  }
  return `No se pudo buscar actualizaciones: ${message}`;
}

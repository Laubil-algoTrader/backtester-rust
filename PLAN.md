# Plan: Emails Profesionales + Licencia Obligatoria

## Resumen
Agregar emails profesionales en eventos clave, obligar licencia (incluso free) en la app de escritorio, y agregar recibos/invoices.

---

## 1. Instalar Resend (servicio de email)

**Archivo:** `lbquant-web/package.json`
- `npm install resend`
- Resend: moderno, 100 emails/día gratis, API simple, funciona perfecto en Vercel
- Env var nueva: `RESEND_API_KEY`

---

## 2. Crear sistema de emails (`lbquant-web/lib/email.ts`)

**Nuevo archivo** con:
- Cliente Resend configurado
- 4 funciones de envío de email con templates HTML profesionales inline:

### Email 1: Bienvenida (registro)
- **Trigger:** `app/api/auth/register/route.ts` (después de crear usuario + licencia free)
- **Contenido:** Logo LBQuant, saludo con nombre, licencia free (LBQ-XXXX-XXXX-XXXX), instrucciones para usar en la app, botón "Descargar App", link al dashboard

### Email 2: Upgrade a Pro (pago exitoso)
- **Trigger:** `app/api/webhook/route.ts` (cuando subscription pasa a `authorized`/`active`)
- **Contenido:** Agradecimiento, nombre, licencia Pro, qué se desbloquea (optimización, exportación), recibo con monto y fecha, botón al dashboard

### Email 3: Confirmación de cancelación
- **Trigger:** `app/api/dashboard/cancel/route.ts` (después de cancelar en MercadoPago)
- **Contenido:** Confirmación, licencia revertida a Free, qué pierde, invitación a volver, botón al dashboard

### Email 4: Recibo/Invoice mensual
- **Trigger:** `app/api/webhook/route.ts` (en cada renovación `authorized`)
- **Contenido:** Número de recibo (fecha-based), monto ARS $15,000, período, método de pago (MercadoPago), datos de licencia

---

## 3. Modificar rutas existentes para enviar emails

### `app/api/auth/register/route.ts`
- Después de crear usuario + free license → llamar `sendWelcomeEmail(name, email, licenseKey)`
- No bloquear el registro si el email falla (fire-and-forget con try/catch)

### `app/api/webhook/route.ts`
- Cuando status es `authorized`/`active` → llamar `sendProUpgradeEmail(name, email, licenseKey, amount, date)`
- También llamar `sendInvoiceEmail(...)` con datos del pago

### `app/api/dashboard/cancel/route.ts`
- Después de cancelar → llamar `sendCancellationEmail(name, email)`

---

## 4. App de escritorio: Licencia obligatoria para TODOS

### `src-tauri/src/license.rs`
- Eliminar el shortcut de "license key vacía = Free tier"
- SIEMPRE hacer la llamada al API de validación, tanto para free como pro
- Si no hay conexión a internet → mostrar error claro pidiendo conectarse

### `src/components/auth/LoginPage.tsx`
- Eliminar botón "Continue Free"
- Hacer el campo "License Key" obligatorio (no opcional)
- Agregar texto: "Don't have a license? Get one free at lbquant-web.vercel.app/register"
- Mantener "Remember me" y auto-load de credenciales guardadas
- Un solo botón "Activate" que valida contra el API

### `src/stores/useAppStore.ts`
- Ajustar si hay lógica que permite skip de licencia

---

## 5. Variables de entorno nuevas

En Vercel + `.env.local`:
- `RESEND_API_KEY` — API key de Resend (obtener en resend.com)
- `RESEND_FROM_EMAIL` — Email remitente (ej: `LBQuant <noreply@lbquant.com>` o dominio verificado en Resend)

---

## 6. Orden de implementación

1. `npm install resend` en lbquant-web
2. Crear `lib/email.ts` con cliente + 4 templates + funciones de envío
3. Modificar `app/api/auth/register/route.ts` → enviar welcome email
4. Modificar `app/api/webhook/route.ts` → enviar pro upgrade + invoice
5. Modificar `app/api/dashboard/cancel/route.ts` → enviar cancellation
6. Modificar `license.rs` → eliminar shortcut de key vacía
7. Modificar `LoginPage.tsx` → eliminar "Continue Free", hacer key required
8. Commit y push ambos repos
9. Agregar `RESEND_API_KEY` en Vercel env vars

---

## 7. Recomendaciones adicionales incluidas

- **Recibo/Invoice** en cada pago (email 4)
- **Número de recibo** auto-generado (formato: LBQ-INV-YYYYMMDD-XXXX)
- Los emails no bloquean operaciones (fire-and-forget)
- Diseño profesional: colores de marca, tipografía limpia, responsive

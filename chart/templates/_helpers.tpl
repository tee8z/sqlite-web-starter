{{/* Keep selectors stable across chart upgrades. */}}
{{- define "sqlite-web-starter.selectorLabels" -}}
app.kubernetes.io/name: sqlite-web-starter
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "sqlite-web-starter.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | quote }}
{{ include "sqlite-web-starter.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/* Preserve the existing account override for installations that already use it. */}}
{{- define "sqlite-web-starter.serviceAccountName" -}}
{{- if .Values.serviceAccountName -}}
{{- .Values.serviceAccountName -}}
{{- else if .Values.serviceAccount.create -}}
{{- default .Release.Name .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end }}

import { typedCall } from "./typed";
import type { StudioApiCall } from "./generated/StudioApiCall";
import type { ApiResults } from "./generated/api-results";
export const apiCall = typedCall<StudioApiCall, ApiResults>("api-studio.api");
import type { StudioWebhookCall } from "./generated/StudioWebhookCall";
import type { WebhookResults } from "./generated/webhook-results";
export const webhookCall = typedCall<StudioWebhookCall, WebhookResults>("api-studio.webhooks");
import type { StudioTransformCall } from "./generated/StudioTransformCall";
import type { TransformResults } from "./generated/transform-results";
export const transformCall = typedCall<StudioTransformCall, TransformResults>("api-studio.transforms");

import type { StoreCall } from "./generated/StoreCall";
import type { StoreResults } from "./generated/store-results";
export const storeCall = typedCall<StoreCall, StoreResults>("api-studio.store");

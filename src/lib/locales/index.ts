// Each namespace file exports { tr, en } where `en` is typed against `tr`'s keys,
// so a missing translation is a type error (caught by `npm run check`).
import common from './common';
import menus from './menus';
import uiLevel from './uiLevel';
import help from './help';
import create from './create';
import fit from './fit';
import kv from './kv';
import hub from './hub';
import settings from './settings';
import connections from './connections';
import perf from './perf';
import onboarding from './onboarding';
import dashboard from './dashboard';
import playground from './playground';
import servers from './servers';
import benchmark from './benchmark';
import importNs from './import';
import caps from './caps';
import params from './params';
import security from './security';
import presets from './presets';
import library from './library';
import embed from './embed';
import developer from './developer';
import recovery from './recovery';
import gateway from './gateway';
import harnessMcp from './harnessMcp';
import integrations from './integrations';
import agentModels from './agentModels';
import analytics from './analytics';
import toolDiag from './toolDiag';
import ollama from './ollama';
import responses from './responses';
import loading from './loading';
import runtimes from './runtimes';
import split from './split';
import compaction from './compaction';
import shell from './shell';
import hfAuth from './hfAuth';
import errors from './errors';
import errorsDownloads from './errorsDownloads';
import errorsIntegrations from './errorsIntegrations';
import errorsRuntime from './errorsRuntime';
import errorsStore from './errorsStore';
import rag from './rag';
import errorsRag from './errorsRag';
import ui from './ui';
import tools from './tools';
import whisper from './whisper';
import sdcpp from './sdcpp';
import tts from './tts';
import sherpaAsr from './sherpaAsr';
import onnx from './onnx';
import pipeline from './pipeline';
import lemonade from './lemonade';
import convert from './convert';
import mistralrs from './mistralrs';
import pyhf from './pyhf';
import engines from './engines';

// Common accessible field-help text.

const namespaces = [uiLevel, menus, help, create, common, fit, kv, hub, settings, connections, perf, onboarding, dashboard, playground, servers, benchmark, importNs, caps, params, security, presets, library, embed, developer, recovery, gateway, harnessMcp, integrations, agentModels, analytics, ollama, responses, loading, runtimes, split, compaction, shell, ui, hfAuth, errors, errorsDownloads, errorsIntegrations, errorsRuntime, errorsStore, rag, errorsRag, tools, toolDiag, whisper, sdcpp, tts, sherpaAsr, onnx, pipeline, lemonade, convert, mistralrs, pyhf, engines] as const;

type UnionToIntersection<U> = (U extends unknown ? (value: U) => void : never) extends (value: infer I) => void ? I : never;
export type Dictionary = UnionToIntersection<(typeof namespaces)[number]['tr']>;

export const tr = Object.assign({}, ...namespaces.map((n) => n.tr)) as Dictionary;
export const en = Object.assign({}, ...namespaces.map((n) => n.en)) as Record<keyof Dictionary, string>;

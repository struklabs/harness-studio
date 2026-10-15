"""Contains all the data models used in inputs/outputs"""

from .active_server_info import ActiveServerInfo
from .adapter_ref import AdapterRef
from .add_local import AddLocal
from .add_runtime_request_github import AddRuntimeRequestGithub
from .add_runtime_request_github_kind import AddRuntimeRequestGithubKind
from .add_runtime_request_local import AddRuntimeRequestLocal
from .add_runtime_request_local_kind import AddRuntimeRequestLocalKind
from .analytics_summary import AnalyticsSummary
from .auto_action_request import AutoActionRequest
from .auto_start_request import AutoStartRequest
from .auto_start_request_endpoint import AutoStartRequestEndpoint
from .automatic_launch import AutomaticLaunch
from .b_tree_map import BTreeMap
from .backend_advice import BackendAdvice
from .backend_info import BackendInfo
from .backend_request import BackendRequest
from .bench_result import BenchResult
from .benchmark_list import BenchmarkList
from .benchmark_ref import BenchmarkRef
from .benchmark_request import BenchmarkRequest
from .benchmark_run import BenchmarkRun
from .benchmark_started import BenchmarkStarted
from .benchmark_status import BenchmarkStatus
from .benchmark_view import BenchmarkView
from .binary_source import BinarySource
from .cancel_compact_response import CancelCompactResponse
from .capability import Capability
from .catalog import Catalog
from .client_endpoint import ClientEndpoint
from .client_info import ClientInfo
from .client_share import ClientShare
from .compact_body import CompactBody
from .compact_response import CompactResponse
from .compaction_marker import CompactionMarker
from .compaction_settings import CompactionSettings
from .compaction_strategy import CompactionStrategy
from .compaction_trigger import CompactionTrigger
from .compatibility_type_0 import CompatibilityType0
from .compatibility_type_0_status import CompatibilityType0Status
from .compatibility_type_1 import CompatibilityType1
from .compatibility_type_1_status import CompatibilityType1Status
from .compatibility_type_2 import CompatibilityType2
from .compatibility_type_2_status import CompatibilityType2Status
from .compatibility_type_3 import CompatibilityType3
from .compatibility_type_3_status import CompatibilityType3Status
from .conversation import Conversation
from .conversation_export import ConversationExport
from .conversation_summary import ConversationSummary
from .conversation_view import ConversationView
from .convert_job import ConvertJob
from .convert_plan import ConvertPlan
from .convert_request import ConvertRequest
from .convert_stage import ConvertStage
from .convert_status import ConvertStatus
from .core_event_type_0 import CoreEventType0
from .core_event_type_0_type import CoreEventType0Type
from .core_event_type_1 import CoreEventType1
from .core_event_type_1_type import CoreEventType1Type
from .core_event_type_2 import CoreEventType2
from .core_event_type_2_type import CoreEventType2Type
from .core_event_type_3 import CoreEventType3
from .core_event_type_3_type import CoreEventType3Type
from .core_event_type_4 import CoreEventType4
from .core_event_type_4_type import CoreEventType4Type
from .core_event_type_5 import CoreEventType5
from .core_event_type_5_type import CoreEventType5Type
from .core_event_type_6 import CoreEventType6
from .core_event_type_6_type import CoreEventType6Type
from .core_event_type_7 import CoreEventType7
from .core_event_type_7_type import CoreEventType7Type
from .core_event_type_8 import CoreEventType8
from .core_event_type_8_type import CoreEventType8Type
from .core_event_type_9 import CoreEventType9
from .core_event_type_9_type import CoreEventType9Type
from .core_event_type_10 import CoreEventType10
from .core_event_type_10_type import CoreEventType10Type
from .core_event_type_11 import CoreEventType11
from .core_event_type_11_type import CoreEventType11Type
from .core_event_type_12 import CoreEventType12
from .core_event_type_12_type import CoreEventType12Type
from .core_event_type_13 import CoreEventType13
from .core_event_type_13_type import CoreEventType13Type
from .core_event_type_14 import CoreEventType14
from .core_event_type_14_type import CoreEventType14Type
from .create_token_request import CreateTokenRequest
from .created_token import CreatedToken
from .daily_point import DailyPoint
from .decision import Decision
from .definition_import import DefinitionImport
from .delete_report import DeleteReport
from .device import Device
from .device_report import DeviceReport
from .direct_client import DirectClient
from .direct_day import DirectDay
from .direct_server import DirectServer
from .direct_usage import DirectUsage
from .download_progress import DownloadProgress
from .download_status_type_0 import DownloadStatusType0
from .download_status_type_1 import DownloadStatusType1
from .download_status_type_2 import DownloadStatusType2
from .download_status_type_3 import DownloadStatusType3
from .download_status_type_4 import DownloadStatusType4
from .download_status_type_5 import DownloadStatusType5
from .download_status_type_6 import DownloadStatusType6
from .draft_spec import DraftSpec
from .driver_compatibility import DriverCompatibility
from .duplicate_request import DuplicateRequest
from .effective_launch_values import EffectiveLaunchValues
from .effective_launch_values_modalities import EffectiveLaunchValuesModalities
from .effective_response import EffectiveResponse
from .endpoint_kind import EndpointKind
from .env_progress import EnvProgress
from .env_status import EnvStatus
from .error_body import ErrorBody
from .error_detail import ErrorDetail
from .error_detail_params import ErrorDetailParams
from .estimate_source import EstimateSource
from .exported_chunk import ExportedChunk
from .exported_conversation import ExportedConversation
from .exported_document import ExportedDocument
from .exported_message import ExportedMessage
from .exported_variant import ExportedVariant
from .exposure_ack_request import ExposureAckRequest
from .failure_kind import FailureKind
from .failure_phase import FailurePhase
from .file_digest import FileDigest
from .fit_category import FitCategory
from .fit_result import FitResult
from .flag_info import FlagInfo
from .fork_hint import ForkHint
from .gateway_endpoint import GatewayEndpoint
from .gateway_settings import GatewaySettings
from .gateway_settings_server_tools import GatewaySettingsServerTools
from .gateway_settings_text_tool_calls import GatewaySettingsTextToolCalls
from .gateway_settings_tool_ignores import GatewaySettingsToolIgnores
from .gateway_status import GatewayStatus
from .gateway_view import GatewayView
from .get_tool_generation_response_200 import GetToolGenerationResponse200
from .gguf_metadata import GgufMetadata
from .github_install import GithubInstall
from .gpu_info import GpuInfo
from .gpu_memory import GpuMemory
from .gpu_summary import GpuSummary
from .gpu_vendor_type_0 import GpuVendorType0
from .gpu_vendor_type_1 import GpuVendorType1
from .gpu_vendor_type_2 import GpuVendorType2
from .gpu_vendor_type_3 import GpuVendorType3
from .gpu_vendor_type_4 import GpuVendorType4
from .hardware_devices_response import HardwareDevicesResponse
from .hardware_summary import HardwareSummary
from .health import Health
from .hf_model_variant import HfModelVariant
from .hf_shard_info import HfShardInfo
from .inference_preset import InferencePreset
from .install_progress import InstallProgress
from .install_progress_response_200_type_0 import InstallProgressResponse200Type0
from .install_request import InstallRequest
from .install_view import InstallView
from .installed import Installed
from .installed_asset import InstalledAsset
from .installed_file import InstalledFile
from .instance_kind import InstanceKind
from .instance_summary import InstanceSummary
from .inventory_tool import InventoryTool
from .keep_alive_type_0 import KeepAliveType0
from .keep_alive_type_1 import KeepAliveType1
from .kv_choice import KvChoice
from .kv_level import KvLevel
from .kv_reason import KvReason
from .last_request import LastRequest
from .launch_config import LaunchConfig
from .lemonade_model import LemonadeModel
from .lemonade_settings import LemonadeSettings
from .lemonade_status import LemonadeStatus
from .lemonade_status_install_progress_type_0 import LemonadeStatusInstallProgressType0
from .library_entry import LibraryEntry
from .load_adaptation import LoadAdaptation
from .load_request import LoadRequest
from .load_response import LoadResponse
from .local_model import LocalModel
from .logs_response import LogsResponse
from .lora_adapter import LoraAdapter
from .lora_response import LoraResponse
from .lora_scale import LoraScale
from .lora_set_request import LoraSetRequest
from .managed_install import ManagedInstall
from .managed_install_request import ManagedInstallRequest
from .managed_runtime_status import ManagedRuntimeStatus
from .managed_runtime_status_backup_type_0 import ManagedRuntimeStatusBackupType0
from .managed_runtime_status_download_type_0 import ManagedRuntimeStatusDownloadType0
from .manifest import Manifest
from .mcp_auth_state import McpAuthState
from .mcp_server_state import McpServerState
from .mcp_server_status import McpServerStatus
from .memory_breakdown import MemoryBreakdown
from .memory_budget import MemoryBudget
from .memory_estimate import MemoryEstimate
from .message_attachment import MessageAttachment
from .message_variant import MessageVariant
from .metrics_response import MetricsResponse
from .model_caps import ModelCaps
from .model_definition import ModelDefinition
from .model_format import ModelFormat
from .model_kind import ModelKind
from .model_list import ModelList
from .model_origin import ModelOrigin
from .model_record import ModelRecord
from .model_ref_type_0 import ModelRefType0
from .model_ref_type_0_kind import ModelRefType0Kind
from .model_ref_type_1 import ModelRefType1
from .model_ref_type_1_kind import ModelRefType1Kind
from .model_request import ModelRequest
from .model_slot import ModelSlot
from .model_stats import ModelStats
from .modelfile_import import ModelfileImport
from .modelfile_imported import ModelfileImported
from .modelfile_mapped import ModelfileMapped
from .modelfile_request import ModelfileRequest
from .modelfile_warning import ModelfileWarning
from .models_response import ModelsResponse
from .npu_info import NpuInfo
from .official_state import OfficialState
from .onnx_verify_report import OnnxVerifyReport
from .ort_variant import OrtVariant
from .otel_protocol import OtelProtocol
from .otel_settings import OtelSettings
from .otel_status import OtelStatus
from .otel_view import OtelView
from .param_info import ParamInfo
from .path_message import PathMessage
from .permission_class import PermissionClass
from .phase import Phase
from .plan_file import PlanFile
from .platform_support import PlatformSupport
from .port_fallback import PortFallback
from .port_holder import PortHolder
from .profile_view import ProfileView
from .pull_progress import PullProgress
from .pull_request import PullRequest
from .pull_response import PullResponse
from .quant_estimate import QuantEstimate
from .queue_hint import QueueHint
from .rag_document import RagDocument
from .rate_limits import RateLimits
from .reasoning_control import ReasoningControl
from .reasoning_settings import ReasoningSettings
from .recommended_backend import RecommendedBackend
from .recorder_status import RecorderStatus
from .recovery_kind import RecoveryKind
from .recovery_option import RecoveryOption
from .recovery_option_overrides import RecoveryOptionOverrides
from .recovery_option_params import RecoveryOptionParams
from .recovery_plan import RecoveryPlan
from .release_pick import ReleasePick
from .release_pick_assets_item import ReleasePickAssetsItem
from .release_query import ReleaseQuery
from .remote_resource import RemoteResource
from .remote_resource_template import RemoteResourceTemplate
from .repair_info import RepairInfo
from .request_detail import RequestDetail
from .request_page import RequestPage
from .request_record import RequestRecord
from .request_row import RequestRow
from .resource_content import ResourceContent
from .resource_list import ResourceList
from .resource_read import ResourceRead
from .responses_settings import ResponsesSettings
from .restart_state import RestartState
from .retry_request import RetryRequest
from .retry_request_overrides import RetryRequestOverrides
from .router_model import RouterModel
from .router_model_request import RouterModelRequest
from .router_model_state import RouterModelState
from .router_models_response import RouterModelsResponse
from .run_status import RunStatus
from .run_tool_request import RunToolRequest
from .run_tool_request_args import RunToolRequestArgs
from .run_tool_response import RunToolResponse
from .runtime_advice import RuntimeAdvice
from .runtime_device import RuntimeDevice
from .runtime_entry import RuntimeEntry
from .runtime_fit import RuntimeFit
from .runtime_flag import RuntimeFlag
from .runtime_source_type_0 import RuntimeSourceType0
from .runtime_source_type_0_kind import RuntimeSourceType0Kind
from .runtime_source_type_1 import RuntimeSourceType1
from .runtime_source_type_1_kind import RuntimeSourceType1Kind
from .runtime_summary import RuntimeSummary
from .runtime_trust import RuntimeTrust
from .runtime_view import RuntimeView
from .sampling_params import SamplingParams
from .save_otel_request import SaveOtelRequest
from .scope import Scope
from .search_provider_kind import SearchProviderKind
from .serve_mode import ServeMode
from .server_counts import ServerCounts
from .server_failure import ServerFailure
from .server_failure_params import ServerFailureParams
from .server_ram import ServerRam
from .server_status_type_0 import ServerStatusType0
from .server_status_type_1 import ServerStatusType1
from .server_status_type_2 import ServerStatusType2
from .server_status_type_3 import ServerStatusType3
from .server_status_type_4 import ServerStatusType4
from .server_tools import ServerTools
from .server_totals import ServerTotals
from .server_view import ServerView
from .set_web_search_settings import SetWebSearchSettings
from .shard_status import ShardStatus
from .slot_advice import SlotAdvice
from .slot_info import SlotInfo
from .slot_origin import SlotOrigin
from .slot_state import SlotState
from .split_suggestion import SplitSuggestion
from .start_request import StartRequest
from .start_request_overrides import StartRequestOverrides
from .start_tool_generation_response_200 import StartToolGenerationResponse200
from .status_response import StatusResponse
from .stored_content import StoredContent
from .support import Support
from .system_hardware_info import SystemHardwareInfo
from .telemetry_history_point import TelemetryHistoryPoint
from .telemetry_snapshot import TelemetrySnapshot
from .tensor_type import TensorType
from .test_export_result import TestExportResult
from .test_otel_request import TestOtelRequest
from .token_info import TokenInfo
from .token_policy import TokenPolicy
from .token_update import TokenUpdate
from .tokenizer_info import TokenizerInfo
from .tool_call_row import ToolCallRow
from .tool_def import ToolDef
from .tool_def_input_schema import ToolDefInputSchema
from .tool_entry import ToolEntry
from .tool_error_code import ToolErrorCode
from .tool_generation_action_response_200 import ToolGenerationActionResponse200
from .tool_limits import ToolLimits
from .tool_listing import ToolListing
from .tool_policy import ToolPolicy
from .tool_result import ToolResult
from .tool_settings import ToolSettings
from .tool_settings_mcp_classes_type_0 import ToolSettingsMcpClassesType0
from .tool_source import ToolSource
from .undo_body import UndoBody
from .undo_response import UndoResponse
from .unload_request import UnloadRequest
from .unload_response import UnloadResponse
from .usage import Usage
from .variant_view import VariantView
from .verified_caps import VerifiedCaps
from .verify_report import VerifyReport
from .verify_request import VerifyRequest
from .version_info import VersionInfo
from .web_search_settings import WebSearchSettings
from .web_search_settings_view import WebSearchSettingsView

__all__ = (
    "ActiveServerInfo",
    "AdapterRef",
    "AddLocal",
    "AddRuntimeRequestGithub",
    "AddRuntimeRequestGithubKind",
    "AddRuntimeRequestLocal",
    "AddRuntimeRequestLocalKind",
    "AnalyticsSummary",
    "AutoActionRequest",
    "AutomaticLaunch",
    "AutoStartRequest",
    "AutoStartRequestEndpoint",
    "BackendAdvice",
    "BackendInfo",
    "BackendRequest",
    "BenchmarkList",
    "BenchmarkRef",
    "BenchmarkRequest",
    "BenchmarkRun",
    "BenchmarkStarted",
    "BenchmarkStatus",
    "BenchmarkView",
    "BenchResult",
    "BinarySource",
    "BTreeMap",
    "CancelCompactResponse",
    "Capability",
    "Catalog",
    "ClientEndpoint",
    "ClientInfo",
    "ClientShare",
    "CompactBody",
    "CompactionMarker",
    "CompactionSettings",
    "CompactionStrategy",
    "CompactionTrigger",
    "CompactResponse",
    "CompatibilityType0",
    "CompatibilityType0Status",
    "CompatibilityType1",
    "CompatibilityType1Status",
    "CompatibilityType2",
    "CompatibilityType2Status",
    "CompatibilityType3",
    "CompatibilityType3Status",
    "Conversation",
    "ConversationExport",
    "ConversationSummary",
    "ConversationView",
    "ConvertJob",
    "ConvertPlan",
    "ConvertRequest",
    "ConvertStage",
    "ConvertStatus",
    "CoreEventType0",
    "CoreEventType0Type",
    "CoreEventType1",
    "CoreEventType10",
    "CoreEventType10Type",
    "CoreEventType11",
    "CoreEventType11Type",
    "CoreEventType12",
    "CoreEventType12Type",
    "CoreEventType13",
    "CoreEventType13Type",
    "CoreEventType14",
    "CoreEventType14Type",
    "CoreEventType1Type",
    "CoreEventType2",
    "CoreEventType2Type",
    "CoreEventType3",
    "CoreEventType3Type",
    "CoreEventType4",
    "CoreEventType4Type",
    "CoreEventType5",
    "CoreEventType5Type",
    "CoreEventType6",
    "CoreEventType6Type",
    "CoreEventType7",
    "CoreEventType7Type",
    "CoreEventType8",
    "CoreEventType8Type",
    "CoreEventType9",
    "CoreEventType9Type",
    "CreatedToken",
    "CreateTokenRequest",
    "DailyPoint",
    "Decision",
    "DefinitionImport",
    "DeleteReport",
    "Device",
    "DeviceReport",
    "DirectClient",
    "DirectDay",
    "DirectServer",
    "DirectUsage",
    "DownloadProgress",
    "DownloadStatusType0",
    "DownloadStatusType1",
    "DownloadStatusType2",
    "DownloadStatusType3",
    "DownloadStatusType4",
    "DownloadStatusType5",
    "DownloadStatusType6",
    "DraftSpec",
    "DriverCompatibility",
    "DuplicateRequest",
    "EffectiveLaunchValues",
    "EffectiveLaunchValuesModalities",
    "EffectiveResponse",
    "EndpointKind",
    "EnvProgress",
    "EnvStatus",
    "ErrorBody",
    "ErrorDetail",
    "ErrorDetailParams",
    "EstimateSource",
    "ExportedChunk",
    "ExportedConversation",
    "ExportedDocument",
    "ExportedMessage",
    "ExportedVariant",
    "ExposureAckRequest",
    "FailureKind",
    "FailurePhase",
    "FileDigest",
    "FitCategory",
    "FitResult",
    "FlagInfo",
    "ForkHint",
    "GatewayEndpoint",
    "GatewaySettings",
    "GatewaySettingsServerTools",
    "GatewaySettingsTextToolCalls",
    "GatewaySettingsToolIgnores",
    "GatewayStatus",
    "GatewayView",
    "GetToolGenerationResponse200",
    "GgufMetadata",
    "GithubInstall",
    "GpuInfo",
    "GpuMemory",
    "GpuSummary",
    "GpuVendorType0",
    "GpuVendorType1",
    "GpuVendorType2",
    "GpuVendorType3",
    "GpuVendorType4",
    "HardwareDevicesResponse",
    "HardwareSummary",
    "Health",
    "HfModelVariant",
    "HfShardInfo",
    "InferencePreset",
    "Installed",
    "InstalledAsset",
    "InstalledFile",
    "InstallProgress",
    "InstallProgressResponse200Type0",
    "InstallRequest",
    "InstallView",
    "InstanceKind",
    "InstanceSummary",
    "InventoryTool",
    "KeepAliveType0",
    "KeepAliveType1",
    "KvChoice",
    "KvLevel",
    "KvReason",
    "LastRequest",
    "LaunchConfig",
    "LemonadeModel",
    "LemonadeSettings",
    "LemonadeStatus",
    "LemonadeStatusInstallProgressType0",
    "LibraryEntry",
    "LoadAdaptation",
    "LoadRequest",
    "LoadResponse",
    "LocalModel",
    "LogsResponse",
    "LoraAdapter",
    "LoraResponse",
    "LoraScale",
    "LoraSetRequest",
    "ManagedInstall",
    "ManagedInstallRequest",
    "ManagedRuntimeStatus",
    "ManagedRuntimeStatusBackupType0",
    "ManagedRuntimeStatusDownloadType0",
    "Manifest",
    "McpAuthState",
    "McpServerState",
    "McpServerStatus",
    "MemoryBreakdown",
    "MemoryBudget",
    "MemoryEstimate",
    "MessageAttachment",
    "MessageVariant",
    "MetricsResponse",
    "ModelCaps",
    "ModelDefinition",
    "ModelfileImport",
    "ModelfileImported",
    "ModelfileMapped",
    "ModelfileRequest",
    "ModelfileWarning",
    "ModelFormat",
    "ModelKind",
    "ModelList",
    "ModelOrigin",
    "ModelRecord",
    "ModelRefType0",
    "ModelRefType0Kind",
    "ModelRefType1",
    "ModelRefType1Kind",
    "ModelRequest",
    "ModelSlot",
    "ModelsResponse",
    "ModelStats",
    "NpuInfo",
    "OfficialState",
    "OnnxVerifyReport",
    "OrtVariant",
    "OtelProtocol",
    "OtelSettings",
    "OtelStatus",
    "OtelView",
    "ParamInfo",
    "PathMessage",
    "PermissionClass",
    "Phase",
    "PlanFile",
    "PlatformSupport",
    "PortFallback",
    "PortHolder",
    "ProfileView",
    "PullProgress",
    "PullRequest",
    "PullResponse",
    "QuantEstimate",
    "QueueHint",
    "RagDocument",
    "RateLimits",
    "ReasoningControl",
    "ReasoningSettings",
    "RecommendedBackend",
    "RecorderStatus",
    "RecoveryKind",
    "RecoveryOption",
    "RecoveryOptionOverrides",
    "RecoveryOptionParams",
    "RecoveryPlan",
    "ReleasePick",
    "ReleasePickAssetsItem",
    "ReleaseQuery",
    "RemoteResource",
    "RemoteResourceTemplate",
    "RepairInfo",
    "RequestDetail",
    "RequestPage",
    "RequestRecord",
    "RequestRow",
    "ResourceContent",
    "ResourceList",
    "ResourceRead",
    "ResponsesSettings",
    "RestartState",
    "RetryRequest",
    "RetryRequestOverrides",
    "RouterModel",
    "RouterModelRequest",
    "RouterModelsResponse",
    "RouterModelState",
    "RunStatus",
    "RuntimeAdvice",
    "RuntimeDevice",
    "RuntimeEntry",
    "RuntimeFit",
    "RuntimeFlag",
    "RuntimeSourceType0",
    "RuntimeSourceType0Kind",
    "RuntimeSourceType1",
    "RuntimeSourceType1Kind",
    "RuntimeSummary",
    "RuntimeTrust",
    "RuntimeView",
    "RunToolRequest",
    "RunToolRequestArgs",
    "RunToolResponse",
    "SamplingParams",
    "SaveOtelRequest",
    "Scope",
    "SearchProviderKind",
    "ServeMode",
    "ServerCounts",
    "ServerFailure",
    "ServerFailureParams",
    "ServerRam",
    "ServerStatusType0",
    "ServerStatusType1",
    "ServerStatusType2",
    "ServerStatusType3",
    "ServerStatusType4",
    "ServerTools",
    "ServerTotals",
    "ServerView",
    "SetWebSearchSettings",
    "ShardStatus",
    "SlotAdvice",
    "SlotInfo",
    "SlotOrigin",
    "SlotState",
    "SplitSuggestion",
    "StartRequest",
    "StartRequestOverrides",
    "StartToolGenerationResponse200",
    "StatusResponse",
    "StoredContent",
    "Support",
    "SystemHardwareInfo",
    "TelemetryHistoryPoint",
    "TelemetrySnapshot",
    "TensorType",
    "TestExportResult",
    "TestOtelRequest",
    "TokenInfo",
    "TokenizerInfo",
    "TokenPolicy",
    "TokenUpdate",
    "ToolCallRow",
    "ToolDef",
    "ToolDefInputSchema",
    "ToolEntry",
    "ToolErrorCode",
    "ToolGenerationActionResponse200",
    "ToolLimits",
    "ToolListing",
    "ToolPolicy",
    "ToolResult",
    "ToolSettings",
    "ToolSettingsMcpClassesType0",
    "ToolSource",
    "UndoBody",
    "UndoResponse",
    "UnloadRequest",
    "UnloadResponse",
    "Usage",
    "VariantView",
    "VerifiedCaps",
    "VerifyReport",
    "VerifyRequest",
    "VersionInfo",
    "WebSearchSettings",
    "WebSearchSettingsView",
)

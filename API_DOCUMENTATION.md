# Cedar-Agent API Documentation

## Table of Contents
1. [Overview](#overview)
2. [Getting Started](#getting-started)
3. [Authentication](#authentication)
4. [API Endpoints](#api-endpoints)
   - [Health Check](#health-check)
   - [Authorization](#authorization)
   - [Policies Management](#policies-management)
   - [Data/Entities Management](#dataentities-management)
   - [Schema Management](#schema-management)
   - [Invalidation Management](#invalidation-management)
5. [Data Models](#data-models)
6. [Error Handling](#error-handling)
7. [Examples](#examples)

---

## Overview

Cedar-Agent is an HTTP server designed to efficiently manage a policy store and a data store with seamless integration to Cedar, Amazon's authorization policy language. It provides RESTful APIs for managing authorization policies, entity data, and schemas.

**Key Features:**
- Policy-based authorization using Cedar policy language
- Entity and attribute management
- Schema validation and management
- Real-time authorization decisions
- OpenAPI/Swagger documentation

**Base URL:** `http://localhost:8180/v1`

**Interactive Documentation:**
- Swagger UI: `http://localhost:8180/swagger-ui/`
- RapiDoc: `http://localhost:8180/rapidoc/`

---

## Getting Started

### Installation & Running

```bash
# Build the project
cargo build --release

# Run with default settings
cargo run

# Run with custom configuration
cargo run -- --port 8080 --authentication "your-api-key"

# Run with file initialization
cargo run -- --schema schema.json --policies policies.json --data data.json
```

### Configuration

Cedar-Agent can be configured via command-line arguments or environment variables:

| CLI Argument | Environment Variable | Description | Default |
|--------------|---------------------|-------------|---------|
| `--port, -p` | `CEDAR_AGENT_PORT` | Server port | 8180 |
| `--addr` | `CEDAR_AGENT_ADDR` | Server address | 0.0.0.0 |
| `--authentication, -a` | `CEDAR_AGENT_AUTHENTICATION` | API key for authentication | None |
| `--log-level, -l` | `CEDAR_AGENT_LOG_LEVEL` | Logging level | Info |
| `--schema, -s` | `CEDAR_AGENT_SCHEMA` | Path to schema JSON file | None |
| `--policies` | `CEDAR_AGENT_POLICIES` | Path to policies JSON file | None |
| `--data, -d` | `CEDAR_AGENT_DATA` | Path to entities JSON file | None |
| (N/A) | `CEDAR_AGENT_INVALIDATION_TARGETS` | JSON array of invalidation targets (in-memory; loaded at startup) | None |
| (N/A) | `CEDAR_AGENT_INVALIDATION_TIMEOUT_MS` | Per-target invalidation timeout (ms) | 5000 |

---

## Authentication

Cedar-Agent supports optional API key authentication. When configured, all API requests must include the API key in the `Authorization` header.

### Setting Up Authentication

```bash
# Via CLI
cargo run -- --authentication "my-secret-api-key"

# Via Environment Variable
export CEDAR_AGENT_AUTHENTICATION="my-secret-api-key"
cargo run
```

### Making Authenticated Requests

```bash
curl -H "Authorization: my-secret-api-key" http://localhost:8180/v1/policies
```

**Note:** If no authentication is configured, the `Authorization` header is optional.

---

## API Endpoints

### Health Check

#### Get Health Status
**Endpoint:** `GET /v1/`

**Description:** Check if the server is running and healthy.

**Authentication:** Optional

**Response:**
- **Status Code:** `204 No Content`

**Example:**
```bash
curl http://localhost:8180/v1/
```

---

### Authorization

#### Check Authorization
**Endpoint:** `POST /v1/is_authorized`

**Description:** Evaluate an authorization request against stored policies and entities to determine if an action is allowed.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "principal": "User::\"admin.1@domain.com\"",
  "action": "Action::\"create\"",
  "resource": "Document::\"cedar-agent.pdf\"",
  "context": {},
  "entities": [],
  "additional_entities": []
}
```

**Parameters:**
- `principal` (optional, string): The entity performing the action (e.g., `User::"alice"`)
- `action` (optional, string): The action being performed (e.g., `Action::"view"`)
- `resource` (optional, string): The resource being accessed (e.g., `Document::"file.pdf"`)
- `context` (optional, object): Additional context for the authorization decision
- `entities` (optional, array): Complete entity set (overrides stored entities if provided)
- `additional_entities` (optional, array): Additional entities to merge with stored entities

**Response:**
```json
{
  "decision": "Allow",
  "diagnostics": {
    "reason": ["admins-policy"],
    "errors": []
  }
}
```

**Response Fields:**
- `decision`: Either `"Allow"` or `"Deny"`
- `diagnostics.reason`: List of policy IDs that contributed to the decision
- `diagnostics.errors`: List of any errors encountered during evaluation

**Example:**
```bash
curl -X POST http://localhost:8180/v1/is_authorized \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "principal": "User::\"admin.1@domain.com\"",
    "action": "Action::\"create\"",
    "resource": "Document::\"cedar-agent.pdf\""
  }'
```

---

### Policies Management

#### List All Policies
**Endpoint:** `GET /v1/policies`

**Description:** Retrieve all stored policies.

**Authentication:** Required (if enabled)

**Response:**
```json
[
  {
    "id": "admins-policy",
    "content": "permit(principal in Role::\"Admin\", action in [Action::\"get\", Action::\"list\"], resource == Document::\"cedar-agent.pdf\");"
  },
  {
    "id": "editors-policy",
    "content": "permit(principal in Role::\"Editor\", action in [Action::\"get\"], resource == Document::\"cedar-agent.pdf\");"
  }
]
```

**Example:**
```bash
curl http://localhost:8180/v1/policies \
  -H "Authorization: my-api-key"
```

---

#### Get Single Policy
**Endpoint:** `GET /v1/policies/{id}`

**Description:** Retrieve a specific policy by its ID.

**Authentication:** Required (if enabled)

**Path Parameters:**
- `id` (required, string): The policy ID

**Response:**
```json
{
  "id": "admins-policy",
  "content": "permit(principal in Role::\"Admin\", action in [Action::\"get\"], resource == Document::\"cedar-agent.pdf\");"
}
```

**Error Response:**
- **Status Code:** `404 Not Found`
```json
{
  "error": "Policy with id 'admins-policy' not found"
}
```

**Example:**
```bash
curl http://localhost:8180/v1/policies/admins-policy \
  -H "Authorization: my-api-key"
```

---

#### Create Policy
**Endpoint:** `POST /v1/policies`

**Description:** Create a new policy. The policy is validated against the current schema.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "id": "new-policy",
  "content": "permit(principal == User::\"alice\", action == Action::\"view\", resource == Document::\"report.pdf\");"
}
```

**Response:**
```json
{
  "id": "new-policy",
  "content": "permit(principal == User::\"alice\", action == Action::\"view\", resource == Document::\"report.pdf\");"
}
```

**Error Responses:**
- **Status Code:** `400 Bad Request` - Invalid policy syntax or schema validation failed
- **Status Code:** `409 Conflict` - Policy with same ID already exists

**Example:**
```bash
curl -X POST http://localhost:8180/v1/policies \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "id": "new-policy",
    "content": "permit(principal == User::\"alice\", action == Action::\"view\", resource == Document::\"report.pdf\");"
  }'
```

---

#### Update All Policies
**Endpoint:** `PUT /v1/policies`

**Description:** Replace all existing policies with a new set. All policies are validated against the current schema.

**Authentication:** Required (if enabled)

**Request Body:**
```json
[
  {
    "id": "policy-1",
    "content": "permit(principal == User::\"alice\", action == Action::\"view\", resource == Document::\"report.pdf\");"
  },
  {
    "id": "policy-2",
    "content": "permit(principal in Role::\"Admin\", action, resource);"
  }
]
```

**Response:**
```json
[
  {
    "id": "policy-1",
    "content": "permit(principal == User::\"alice\", action == Action::\"view\", resource == Document::\"report.pdf\");"
  },
  {
    "id": "policy-2",
    "content": "permit(principal in Role::\"Admin\", action, resource);"
  }
]
```

**Error Response:**
- **Status Code:** `400 Bad Request` - Invalid policy syntax or validation failed

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/policies \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '[
    {
      "id": "policy-1",
      "content": "permit(principal == User::\"alice\", action == Action::\"view\", resource == Document::\"report.pdf\");"
    }
  ]'
```

---

#### Update Single Policy
**Endpoint:** `PUT /v1/policies/{id}`

**Description:** Update a specific policy by its ID.

**Authentication:** Required (if enabled)

**Path Parameters:**
- `id` (required, string): The policy ID to update

**Request Body:**
```json
{
  "content": "permit(principal in Role::\"Admin\", action, resource);"
}
```

**Response:**
```json
{
  "id": "admins-policy",
  "content": "permit(principal in Role::\"Admin\", action, resource);"
}
```

**Error Response:**
- **Status Code:** `400 Bad Request` - Invalid policy syntax or validation failed

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/policies/admins-policy \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "content": "permit(principal in Role::\"Admin\", action, resource);"
  }'
```

---

#### Delete Policy
**Endpoint:** `DELETE /v1/policies/{id}`

**Description:** Delete a specific policy by its ID.

**Authentication:** Required (if enabled)

**Path Parameters:**
- `id` (required, string): The policy ID to delete

**Response:**
- **Status Code:** `204 No Content`

**Error Response:**
- **Status Code:** `404 Not Found` - Policy not found

**Example:**
```bash
curl -X DELETE http://localhost:8180/v1/policies/admins-policy \
  -H "Authorization: my-api-key"
```

---

### Data/Entities Management

#### Get All Entities
**Endpoint:** `GET /v1/data`

**Description:** Retrieve all stored entities.

**Authentication:** Required (if enabled)

**Response:**
```json
[
  {
    "uid": {
      "id": "admin.1@domain.com",
      "type": "User"
    },
    "attrs": {
      "department": "Engineering"
    },
    "parents": [
      {
        "id": "Admin",
        "type": "Role"
      }
    ]
  },
  {
    "uid": {
      "id": "cedar-agent.pdf",
      "type": "Document"
    },
    "attrs": {},
    "parents": []
  }
]
```

**Example:**
```bash
curl http://localhost:8180/v1/data \
  -H "Authorization: my-api-key"
```

---

#### Update All Entities
**Endpoint:** `PUT /v1/data`

**Description:** Replace all entities with a new set. Entities are validated against the current schema.

**Authentication:** Required (if enabled)

**Request Body:**
```json
[
  {
    "uid": {
      "id": "alice",
      "type": "User"
    },
    "attrs": {
      "department": "Engineering"
    },
    "parents": [
      {
        "id": "Developers",
        "type": "Role"
      }
    ]
  }
]
```

**Response:**
```json
[
  {
    "uid": {
      "id": "alice",
      "type": "User"
    },
    "attrs": {
      "department": "Engineering"
    },
    "parents": [
      {
        "id": "Developers",
        "type": "Role"
      }
    ]
  }
]
```

**Error Response:**
- **Status Code:** `400 Bad Request` - Invalid entity format or schema validation failed

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/data \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '[
    {
      "uid": {"id": "alice", "type": "User"},
      "attrs": {"department": "Engineering"},
      "parents": [{"id": "Developers", "type": "Role"}]
    }
  ]'
```

---

#### Delete All Entities
**Endpoint:** `DELETE /v1/data`

**Description:** Delete all entities from the data store.

**Authentication:** Required (if enabled)

**Response:**
- **Status Code:** `204 No Content`

**Example:**
```bash
curl -X DELETE http://localhost:8180/v1/data \
  -H "Authorization: my-api-key"
```

---

#### Add New Entity
**Endpoint:** `PUT /v1/data/entity`

**Description:** Add a new entity to the data store. The entity is created with empty attributes and no parents.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "entity_id": "alice",
  "entity_type": "User"
}
```

**Response:**
```json
[
  {
    "uid": {
      "id": "alice",
      "type": "User"
    },
    "attrs": {},
    "parents": []
  }
]
```

**Error Response:**
- **Status Code:** `409 Conflict` - Entity already exists

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/data/entity \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "entity_id": "alice",
    "entity_type": "User"
  }'
```

---

#### Add Single Entity
**Endpoint:** `PUT /v1/data/single`

**Description:** Add a single entity to the existing entity set.

**Authentication:** Required (if enabled)

**Request Body:**
```json
[
  {
    "uid": {
      "id": "bob",
      "type": "User"
    },
    "attrs": {
      "department": "Sales"
    },
    "parents": []
  }
]
```

**Response:**
```json
[
  {
    "uid": {
      "id": "bob",
      "type": "User"
    },
    "attrs": {
      "department": "Sales"
    },
    "parents": []
  }
]
```

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/data/single \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '[
    {
      "uid": {"id": "bob", "type": "User"},
      "attrs": {"department": "Sales"},
      "parents": []
    }
  ]'
```

---

#### Update Single Entity
**Endpoint:** `PUT /v1/data/single/{entity_id}`

**Description:** Update or create a specific entity by ID. If the entity exists, it's updated; otherwise, it's created.

**Authentication:** Required (if enabled)

**Path Parameters:**
- `entity_id` (required, string): The entity ID

**Request Body:**
```json
[
  {
    "uid": {
      "id": "alice",
      "type": "User"
    },
    "attrs": {
      "department": "Engineering",
      "level": "Senior"
    },
    "parents": [
      {
        "id": "Developers",
        "type": "Role"
      }
    ]
  }
]
```

**Response:**
```json
{
  "uid": {
    "id": "alice",
    "type": "User"
  },
  "attrs": {
    "department": "Engineering",
    "level": "Senior"
  },
  "parents": [
    {
      "id": "Developers",
      "type": "Role"
    }
  ]
}
```

**Error Response:**
- **Status Code:** `400 Bad Request` - Multiple entities provided or validation failed

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/data/single/alice \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '[
    {
      "uid": {"id": "alice", "type": "User"},
      "attrs": {"department": "Engineering"},
      "parents": []
    }
  ]'
```

---

#### Delete Single Entity
**Endpoint:** `DELETE /v1/data/single/{entity_id}`

**Description:** Delete a specific entity by its ID.

**Authentication:** Required (if enabled)

**Path Parameters:**
- `entity_id` (required, string): The entity ID to delete

**Response:**
- **Status Code:** `204 No Content`

**Example:**
```bash
curl -X DELETE http://localhost:8180/v1/data/single/alice \
  -H "Authorization: my-api-key"
```

---

#### Update Entity Attribute
**Endpoint:** `PUT /v1/data/attribute`

**Description:** Add or update a specific attribute for an entity.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "entity_id": "alice",
  "attribute_name": "department",
  "attribute_value": "Engineering"
}
```

**Response:**
```json
{
  "uid": {
    "id": "alice",
    "type": "User"
  },
  "attrs": {
    "department": "Engineering"
  },
  "parents": []
}
```

**Error Response:**
- **Status Code:** `400 Bad Request` - Entity not found

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/data/attribute \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "entity_id": "alice",
    "attribute_name": "department",
    "attribute_value": "Engineering"
  }'
```

---

#### Delete Entity Attribute
**Endpoint:** `DELETE /v1/data/attribute`

**Description:** Remove a specific attribute from an entity.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "entity_id": "alice",
  "attribute_name": "department"
}
```

**Response:**
```json
{
  "uid": {
    "id": "alice",
    "type": "User"
  },
  "attrs": {},
  "parents": []
}
```

**Error Response:**
- **Status Code:** `400 Bad Request` - Entity not found

**Example:**
```bash
curl -X DELETE http://localhost:8180/v1/data/attribute \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "entity_id": "alice",
    "attribute_name": "department"
  }'
```

---

### Schema Management

#### Get Schema
**Endpoint:** `GET /v1/schema`

**Description:** Retrieve the current schema definition.

**Authentication:** Required (if enabled)

**Response:**
```json
{
  "": {
    "entityTypes": {
      "User": {
        "shape": {
          "type": "Record",
          "attributes": {
            "department": {
              "type": "String",
              "required": false
            }
          }
        },
        "memberOfTypes": ["Role"]
      },
      "Role": {
        "shape": {
          "type": "Record",
          "attributes": {}
        }
      },
      "Document": {
        "shape": {
          "type": "Record",
          "attributes": {}
        }
      }
    },
    "actions": {
      "view": {
        "appliesTo": {
          "principalTypes": ["User", "Role"],
          "resourceTypes": ["Document"]
        }
      }
    }
  }
}
```

**Example:**
```bash
curl http://localhost:8180/v1/schema \
  -H "Authorization: my-api-key"
```

---

#### Update Schema
**Endpoint:** `PUT /v1/schema`

**Description:** Update the schema. Validates that all existing policies and entities are compatible with the new schema.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "": {
    "entityTypes": {
      "User": {
        "shape": {
          "type": "Record",
          "attributes": {
            "department": {
              "type": "String",
              "required": true
            }
          }
        },
        "memberOfTypes": ["Role"]
      }
    },
    "actions": {
      "view": {
        "appliesTo": {
          "principalTypes": ["User"],
          "resourceTypes": ["Document"]
        }
      }
    }
  }
}
```

**Response:**
```json
{
  "": {
    "entityTypes": {
      "User": {
        "shape": {
          "type": "Record",
          "attributes": {
            "department": {
              "type": "String",
              "required": true
            }
          }
        },
        "memberOfTypes": ["Role"]
      }
    },
    "actions": {
      "view": {
        "appliesTo": {
          "principalTypes": ["User"],
          "resourceTypes": ["Document"]
        }
      }
    }
  }
}
```

**Error Responses:**
- **Status Code:** `400 Bad Request` - Invalid schema format
- **Status Code:** `400 Bad Request` - Existing policies invalid with new schema
- **Status Code:** `400 Bad Request` - Existing entities invalid with new schema

**Example:**
```bash
curl -X PUT http://localhost:8180/v1/schema \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "": {
      "entityTypes": {
        "User": {
          "shape": {
            "type": "Record",
            "attributes": {}
          }
        }
      },
      "actions": {}
    }
  }'
```

---

#### Delete Schema
**Endpoint:** `DELETE /v1/schema`

**Description:** Delete the current schema.

**Authentication:** Required (if enabled)

**Response:**
- **Status Code:** `204 No Content`

**Example:**
```bash
curl -X DELETE http://localhost:8180/v1/schema \
  -H "Authorization: my-api-key"
```

---

#### Add User Attribute to Schema
**Endpoint:** `POST /v1/schema/user/attribute`

**Description:** Add a new attribute definition to the User entity type in the schema.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "name": "email",
  "type": "String",
  "required": false
}
```

**Response:**
```json
{
  "shape": {
    "type": "Record",
    "attributes": {
      "email": {
        "type": "String",
        "required": false
      }
    }
  },
  "memberOfTypes": ["Role"]
}
```

**Error Response:**
- **Status Code:** `400 Bad Request` - User entity type not found in schema or validation failed

**Example:**
```bash
curl -X POST http://localhost:8180/v1/schema/user/attribute \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "name": "email",
    "type": "String",
    "required": false
  }'
```

---

#### Add Resource Attribute to Schema
**Endpoint:** `POST /v1/schema/resource/attribute`

**Description:** Add a new attribute definition to the Table (resource) entity type in the schema.

**Authentication:** Required (if enabled)

**Request Body:**
```json
{
  "name": "owner",
  "type": "String",
  "required": true
}
```

**Response:**
```json
{
  "shape": {
    "type": "Record",
    "attributes": {
      "owner": {
        "type": "String",
        "required": true
      }
    }
  }
}
```

**Error Response:**
- **Status Code:** `400 Bad Request` - Table entity type not found in schema or validation failed

**Example:**
```bash
curl -X POST http://localhost:8180/v1/schema/resource/attribute \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "name": "owner",
    "type": "String",
    "required": true
  }'
```

---

#### Delete User Attribute from Schema
**Endpoint:** `DELETE /v1/schema/user/attribute/{attr_name}`

**Description:** Remove an attribute definition from the User entity type. Validates that existing policies and entities remain valid.

**Authentication:** Required (if enabled)

**Path Parameters:**
- `attr_name` (required, string): The attribute name to delete

**Response:**
- **Status Code:** `204 No Content`

**Error Responses:**
- **Status Code:** `400 Bad Request` - User entity type not found
- **Status Code:** `400 Bad Request` - Existing policies invalid without this attribute
- **Status Code:** `400 Bad Request` - Existing entities invalid without this attribute

**Example:**
```bash
curl -X DELETE http://localhost:8180/v1/schema/user/attribute/email \
  -H "Authorization: my-api-key"
```

---

#### Delete Resource Attribute from Schema
**Endpoint:** `DELETE /v1/schema/resource/attribute/{attr_name}`

**Description:** Remove an attribute definition from the Table (resource) entity type. Validates that existing policies and entities remain valid.

**Authentication:** Required (if enabled)

**Path Parameters:**
- `attr_name` (required, string): The attribute name to delete

**Response:**
- **Status Code:** `204 No Content`

**Error Responses:**
- **Status Code:** `400 Bad Request` - Table entity type not found
- **Status Code:** `400 Bad Request` - Existing policies invalid without this attribute
- **Status Code:** `400 Bad Request` - Existing entities invalid without this attribute

**Example:**
```bash
curl -X DELETE http://localhost:8180/v1/schema/resource/attribute/owner \
  -H "Authorization: my-api-key"
```

---

## Invalidation Management

Cedar-Agent can push authorization-cache invalidations to configured database targets when policies/schema/entities change.

Key behaviors:
- Strict mode: for policy/schema/data mutations, the request fails if any configured invalidation target fails; the in-memory change is rolled back.
- DB-origin entity sync: database extensions that sync entities to Cedar-Agent should set `X-Cedar-Write-Origin: db-entity-sync` so Cedar-Agent skips invalidation (the DB already flushes its own cache on DDL sync).

### List Invalidation Targets

**Endpoint:** `GET /v1/invalidation/targets`

**Authentication:** Required (if enabled)

**Response:** JSON array of targets.

### Replace Invalidation Targets

**Endpoint:** `PUT /v1/invalidation/targets`

**Authentication:** Required (if enabled)

**Request Body:** JSON array of targets:
```json
[
  {"name":"pg-primary-1","kind":"postgres","dsn":"postgres://...","enabled":true},
  {"name":"mysql-1","kind":"mysql","dsn":"mysql://...","enabled":true}
]
```

**Notes:**
- Targets are stored in-memory for the running cedar-agent instance. Persisted configuration may be added later.
- A future enhancement is revision/versioning to provide correctness even if push invalidation is missed.

---

## Data Models

### Policy

Represents a Cedar authorization policy.

```json
{
  "id": "string",
  "content": "string"
}
```

**Fields:**
- `id` (string, required): Unique identifier for the policy
- `content` (string, required): Cedar policy language statement

**Example:**
```json
{
  "id": "admin-access",
  "content": "permit(principal in Role::\"Admin\", action, resource);"
}
```

---

### Entity

Represents an entity in the authorization system.

```json
{
  "uid": {
    "id": "string",
    "type": "string"
  },
  "attrs": {
    "key": "value"
  },
  "parents": [
    {
      "id": "string",
      "type": "string"
    }
  ]
}
```

**Fields:**
- `uid` (object, required): Unique identifier for the entity
  - `id` (string): Entity ID
  - `type` (string): Entity type (e.g., "User", "Role", "Document")
- `attrs` (object): Key-value pairs of entity attributes
- `parents` (array): List of parent entities (for hierarchical relationships)

**Example:**
```json
{
  "uid": {
    "id": "alice@example.com",
    "type": "User"
  },
  "attrs": {
    "department": "Engineering",
    "level": "Senior"
  },
  "parents": [
    {
      "id": "Developers",
      "type": "Role"
    }
  ]
}
```

---

### Schema

Represents the Cedar schema defining entity types, attributes, and actions.

```json
{
  "": {
    "entityTypes": {
      "EntityTypeName": {
        "shape": {
          "type": "Record",
          "attributes": {
            "attrName": {
              "type": "String|Long|Boolean",
              "required": true|false
            }
          }
        },
        "memberOfTypes": ["ParentType"]
      }
    },
    "actions": {
      "actionName": {
        "appliesTo": {
          "principalTypes": ["EntityType"],
          "resourceTypes": ["EntityType"]
        }
      }
    }
  }
}
```

**Structure:**
- `entityTypes`: Defines entity types and their attributes
  - `shape`: Structure definition for the entity
  - `attributes`: Attribute definitions with type and required flag
  - `memberOfTypes`: Parent entity types
- `actions`: Defines available actions
  - `appliesTo`: Specifies which principal and resource types the action applies to

---

### Authorization Request

```json
{
  "principal": "User::\"alice\"",
  "action": "Action::\"view\"",
  "resource": "Document::\"report.pdf\"",
  "context": {},
  "entities": [],
  "additional_entities": []
}
```

**Fields:**
- `principal` (string, optional): The entity performing the action
- `action` (string, optional): The action being performed
- `resource` (string, optional): The resource being accessed
- `context` (object, optional): Additional contextual information
- `entities` (array, optional): Complete entity set (overrides stored entities)
- `additional_entities` (array, optional): Additional entities to merge with stored entities

---

### Authorization Response

```json
{
  "decision": "Allow",
  "diagnostics": {
    "reason": ["policy-id-1", "policy-id-2"],
    "errors": []
  }
}
```

**Fields:**
- `decision` (string): Either "Allow" or "Deny"
- `diagnostics` (object): Additional information about the decision
  - `reason` (array): Policy IDs that contributed to the decision
  - `errors` (array): Any errors encountered during evaluation

---

### Attribute Schema

```json
{
  "name": "string",
  "type": "String|Long|Boolean",
  "required": true|false
}
```

**Fields:**
- `name` (string): Attribute name
- `type` (string): Data type (String, Long, or Boolean)
- `required` (boolean): Whether the attribute is required

---

## Error Handling

Cedar-Agent uses standard HTTP status codes and returns error responses in JSON format.

### Error Response Format

```json
{
  "error": "Error message describing what went wrong"
}
```

### HTTP Status Codes

| Status Code | Description |
|-------------|-------------|
| `200 OK` | Request successful |
| `204 No Content` | Request successful, no content to return |
| `400 Bad Request` | Invalid request format or validation failed |
| `401 Unauthorized` | Missing or invalid authentication |
| `404 Not Found` | Resource not found |
| `409 Conflict` | Resource already exists |
| `500 Internal Server Error` | Server error |

### Common Error Scenarios

#### Invalid Authentication
```bash
# Request
curl http://localhost:8180/v1/policies

# Response (401)
{
  "error": "Unauthorized"
}
```

#### Policy Not Found
```bash
# Request
curl http://localhost:8180/v1/policies/nonexistent

# Response (404)
{
  "error": "Policy with id 'nonexistent' not found"
}
```

#### Invalid Policy Syntax
```bash
# Request
curl -X POST http://localhost:8180/v1/policies \
  -H "Content-Type: application/json" \
  -d '{"id": "test", "content": "invalid syntax"}'

# Response (400)
{
  "error": "Parse error: unexpected token..."
}
```

#### Schema Validation Failed
```bash
# Request
curl -X PUT http://localhost:8180/v1/schema \
  -H "Content-Type: application/json" \
  -d '{"": {"entityTypes": {}}}'

# Response (400)
{
  "error": "Existing policies invalid with the new schema: ..."
}
```

---

## Examples

### Complete Workflow Example

#### 1. Initialize Schema

```bash
curl -X PUT http://localhost:8180/v1/schema \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "": {
      "entityTypes": {
        "User": {
          "shape": {
            "type": "Record",
            "attributes": {
              "department": {
                "type": "String",
                "required": false
              }
            }
          },
          "memberOfTypes": ["Role"]
        },
        "Role": {
          "shape": {
            "type": "Record",
            "attributes": {}
          }
        },
        "Document": {
          "shape": {
            "type": "Record",
            "attributes": {}
          }
        }
      },
      "actions": {
        "view": {
          "appliesTo": {
            "principalTypes": ["User", "Role"],
            "resourceTypes": ["Document"]
          }
        },
        "edit": {
          "appliesTo": {
            "principalTypes": ["User", "Role"],
            "resourceTypes": ["Document"]
          }
        }
      }
    }
  }'
```

#### 2. Create Policies

```bash
# Admin policy
curl -X POST http://localhost:8180/v1/policies \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "id": "admin-full-access",
    "content": "permit(principal in Role::\"Admin\", action, resource);"
  }'

# Editor policy
curl -X POST http://localhost:8180/v1/policies \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "id": "editor-access",
    "content": "permit(principal in Role::\"Editor\", action in [Action::\"view\", Action::\"edit\"], resource);"
  }'
```

#### 3. Add Entities

```bash
curl -X PUT http://localhost:8180/v1/data \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '[
    {
      "uid": {"id": "alice", "type": "User"},
      "attrs": {"department": "Engineering"},
      "parents": [{"id": "Admin", "type": "Role"}]
    },
    {
      "uid": {"id": "bob", "type": "User"},
      "attrs": {"department": "Marketing"},
      "parents": [{"id": "Editor", "type": "Role"}]
    },
    {
      "uid": {"id": "Admin", "type": "Role"},
      "attrs": {},
      "parents": []
    },
    {
      "uid": {"id": "Editor", "type": "Role"},
      "attrs": {},
      "parents": []
    },
    {
      "uid": {"id": "report.pdf", "type": "Document"},
      "attrs": {},
      "parents": []
    },
    {
      "uid": {"id": "view", "type": "Action"},
      "attrs": {},
      "parents": []
    },
    {
      "uid": {"id": "edit", "type": "Action"},
      "attrs": {},
      "parents": []
    }
  ]'
```

#### 4. Check Authorization

```bash
# Alice (Admin) trying to edit - Should be allowed
curl -X POST http://localhost:8180/v1/is_authorized \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "principal": "User::\"alice\"",
    "action": "Action::\"edit\"",
    "resource": "Document::\"report.pdf\""
  }'

# Response:
# {
#   "decision": "Allow",
#   "diagnostics": {
#     "reason": ["admin-full-access"],
#     "errors": []
#   }
# }

# Bob (Editor) trying to view - Should be allowed
curl -X POST http://localhost:8180/v1/is_authorized \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "principal": "User::\"bob\"",
    "action": "Action::\"view\"",
    "resource": "Document::\"report.pdf\""
  }'

# Response:
# {
#   "decision": "Allow",
#   "diagnostics": {
#     "reason": ["editor-access"],
#     "errors": []
#   }
# }
```

### Dynamic Schema Management Example

```bash
# Add new attribute to User schema
curl -X POST http://localhost:8180/v1/schema/user/attribute \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "name": "email",
    "type": "String",
    "required": true
  }'

# Update entity with new attribute
curl -X PUT http://localhost:8180/v1/data/attribute \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "entity_id": "alice",
    "attribute_name": "email",
    "attribute_value": "alice@example.com"
  }'

# Remove attribute from schema
curl -X DELETE http://localhost:8180/v1/schema/user/attribute/email \
  -H "Authorization: my-api-key"
```

### Context-Based Authorization Example

```bash
# Authorization with context
curl -X POST http://localhost:8180/v1/is_authorized \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "principal": "User::\"alice\"",
    "action": "Action::\"view\"",
    "resource": "Document::\"report.pdf\"",
    "context": {
      "ip": "192.168.1.1",
      "time": "2025-10-08T10:00:00Z"
    }
  }'
```

### Additional Entities Example

```bash
# Provide additional entities in authorization request
curl -X POST http://localhost:8180/v1/is_authorized \
  -H "Content-Type: application/json" \
  -H "Authorization: my-api-key" \
  -d '{
    "principal": "User::\"charlie\"",
    "action": "Action::\"view\"",
    "resource": "Document::\"temp.pdf\"",
    "additional_entities": [
      {
        "uid": {"id": "charlie", "type": "User"},
        "attrs": {},
        "parents": [{"id": "Guest", "type": "Role"}]
      },
      {
        "uid": {"id": "Guest", "type": "Role"},
        "attrs": {},
        "parents": []
      },
      {
        "uid": {"id": "temp.pdf", "type": "Document"},
        "attrs": {},
        "parents": []
      }
    ]
  }'
```

---

## Best Practices

### 1. Schema-First Approach
Always define your schema before creating policies and entities. This ensures proper validation and prevents runtime errors.

### 2. Use Hierarchical Entities
Leverage the `parents` relationship to create role hierarchies and simplify policy management.

### 3. Policy Naming
Use descriptive policy IDs that indicate their purpose (e.g., `admin-full-access`, `viewer-read-only`).

### 4. Batch Operations
When initializing or updating multiple resources, use batch endpoints (`PUT /v1/policies`, `PUT /v1/data`) for better performance.

### 5. Validation
Always validate policies and entities against the schema before deployment to avoid authorization failures.

### 6. Error Handling
Implement proper error handling in your client applications to gracefully handle validation failures and authorization denials.

### 7. Security
- Always use authentication in production environments
- Use HTTPS for all API communications
- Rotate API keys regularly
- Implement proper logging and monitoring

---

## Appendix

### Cedar Policy Language Basics

Cedar policies follow this general format:

```
permit|forbid (
  principal [== | in] <Principal>,
  action [== | in] <Action>,
  resource [== | in] <Resource>
)
[when { <condition> }]
[unless { <condition> }];
```

**Examples:**

```cedar
// Allow all actions for admins
permit(principal in Role::"Admin", action, resource);

// Allow specific actions
permit(
  principal == User::"alice",
  action in [Action::"view", Action::"edit"],
  resource == Document::"report.pdf"
);

// Conditional policy
permit(
  principal,
  action == Action::"view",
  resource
)
when { resource.public == true };
```

### File-Based Initialization

You can initialize Cedar-Agent with JSON files:

```bash
# Start with pre-loaded data
cargo run -- \
  --schema examples/schema.json \
  --policies examples/policies.json \
  --data examples/data.json
```

**schema.json:**
```json
{
  "": {
    "entityTypes": {
      "User": {...}
    },
    "actions": {...}
  }
}
```

**policies.json:**
```json
[
  {
    "id": "policy-1",
    "content": "permit(...);"
  }
]
```

**data.json:**
```json
[
  {
    "uid": {"id": "alice", "type": "User"},
    "attrs": {},
    "parents": []
  }
]
```

### Environment Variables Reference

```bash
# Authentication
export CEDAR_AGENT_AUTHENTICATION="your-api-key"

# Server configuration
export CEDAR_AGENT_PORT=8180
export CEDAR_AGENT_ADDR="0.0.0.0"

# Logging
export CEDAR_AGENT_LOG_LEVEL="info"  # trace, debug, info, warn, error

# Initial data
export CEDAR_AGENT_SCHEMA="/path/to/schema.json"
export CEDAR_AGENT_POLICIES="/path/to/policies.json"
export CEDAR_AGENT_DATA="/path/to/data.json"
```

---

## Support & Resources

- **GitHub Repository:** [https://github.com/permitio/cedar-agent](https://github.com/permitio/cedar-agent)
- **Cedar Documentation:** [https://www.cedarpolicy.com/](https://www.cedarpolicy.com/)
- **License:** Apache-2.0

---

**Document Version:** 1.0  
**Last Updated:** October 8, 2025  
**Cedar-Agent Version:** 0.2.0

// AMP Channel Lifecycle Harness
//
// Drives the full channel lifecycle: create → invite → accept/join →
// send/receive → leave → key rotation → send/receive.

module harness_amp_channel {
  import amp_channel from "../amp/channel"

  var phase: int
  var action_name: str

  action init = all {
    amp_channel::init,
    phase' = 0,
    action_name' = "init"
  }

  action step = {
    if (phase == 0)
      all {
        phase == 0,
        amp_channel::createChannel("bob", "guardians"),
        phase' = 1,
        action_name' = "createChannel"
      }
    else if (phase == 1)
      all {
        phase == 1,
        amp_channel::inviteMember("bob", "guardians", "alice"),
        phase' = 2,
        action_name' = "inviteMember"
      }
    else if (phase == 2)
      all {
        phase == 2,
        amp_channel::inviteMember("bob", "guardians", "carol"),
        phase' = 3,
        action_name' = "inviteMember"
      }
    else if (phase == 3)
      all {
        phase == 3,
        amp_channel::acceptInvite("alice", "guardians"),
        phase' = 4,
        action_name' = "acceptInvite"
      }
    else if (phase == 4)
      all {
        phase == 4,
        amp_channel::joinChannel("alice", "guardians"),
        phase' = 5,
        action_name' = "joinChannel"
      }
    else if (phase == 5)
      all {
        phase == 5,
        amp_channel::acceptInvite("carol", "guardians"),
        phase' = 6,
        action_name' = "acceptInvite"
      }
    else if (phase == 6)
      all {
        phase == 6,
        amp_channel::joinChannel("carol", "guardians"),
        phase' = 7,
        action_name' = "joinChannel"
      }
    else if (phase == 7)
      all {
        phase == 7,
        amp_channel::sendMessage("bob", "guardians", "msg-1"),
        phase' = 8,
        action_name' = "sendMessage"
      }
    else if (phase == 8)
      all {
        phase == 8,
        amp_channel::receiveMessage("alice", "guardians", "msg-1"),
        phase' = 9,
        action_name' = "receiveMessage"
      }
    else if (phase == 9)
      all {
        phase == 9,
        amp_channel::receiveMessage("carol", "guardians", "msg-1"),
        phase' = 10,
        action_name' = "receiveMessage"
      }
    else if (phase == 10)
      all {
        phase == 10,
        amp_channel::leaveChannel("carol", "guardians"),
        phase' = 11,
        action_name' = "leaveChannel"
      }
    else if (phase == 11)
      all {
        phase == 11,
        amp_channel::sendMessage("bob", "guardians", "msg-2"),
        phase' = 12,
        action_name' = "sendMessage"
      }
    else if (phase == 12)
      all {
        phase == 12,
        amp_channel::receiveMessage("alice", "guardians", "msg-2"),
        phase' = 13,
        action_name' = "receiveMessage"
      }
    else if (phase == 13)
      all {
        // Key rotation on leave
        assert(amp_channel::channels.get("guardians").epoch == 1),
        // Carol should not receive messages after leaving
        assert(not(amp_channel::deliveries.get("msg-2").contains("carol"))),
        amp_channel::channels' = amp_channel::channels,
        amp_channel::invitations' = amp_channel::invitations,
        amp_channel::messages' = amp_channel::messages,
        amp_channel::deliveries' = amp_channel::deliveries,
        amp_channel::actor' = amp_channel::actor,
        amp_channel::channel' = amp_channel::channel,
        amp_channel::member' = amp_channel::member,
        amp_channel::message' = amp_channel::message,
        phase' = 14,
        action_name' = "assertInvariant"
      }
    else all {
      false,
      amp_channel::channels' = amp_channel::channels,
      amp_channel::invitations' = amp_channel::invitations,
      amp_channel::messages' = amp_channel::messages,
      amp_channel::deliveries' = amp_channel::deliveries,
      amp_channel::actor' = amp_channel::actor,
      amp_channel::channel' = amp_channel::channel,
      amp_channel::member' = amp_channel::member,
      amp_channel::message' = amp_channel::message,
      phase' = phase,
      action_name' = action_name
    }
  }
}

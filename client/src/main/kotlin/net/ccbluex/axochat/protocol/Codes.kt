package net.ccbluex.axochat.protocol

import kotlinx.serialization.Serializable

// value classes rather than enums, so names from a newer server decode instead of failing the packet

@Serializable
@JvmInline
public value class ErrorCode(public val code: String) {
    override fun toString(): String = code

    public companion object {
        public val NotSupported: ErrorCode = ErrorCode("NotSupported")
        public val LoginFailed: ErrorCode = ErrorCode("LoginFailed")
        public val NotLoggedIn: ErrorCode = ErrorCode("NotLoggedIn")
        public val AlreadyLoggedIn: ErrorCode = ErrorCode("AlreadyLoggedIn")
        public val MojangRequestMissing: ErrorCode = ErrorCode("MojangRequestMissing")
        public val NotPermitted: ErrorCode = ErrorCode("NotPermitted")
        public val NotBanned: ErrorCode = ErrorCode("NotBanned")
        public val Banned: ErrorCode = ErrorCode("Banned")
        public val RateLimited: ErrorCode = ErrorCode("RateLimited")
        public val PrivateMessageNotAccepted: ErrorCode = ErrorCode("PrivateMessageNotAccepted")
        public val EmptyMessage: ErrorCode = ErrorCode("EmptyMessage")
        public val MessageTooLong: ErrorCode = ErrorCode("MessageTooLong")
        public val InvalidCharacter: ErrorCode = ErrorCode("InvalidCharacter")
        public val InvalidId: ErrorCode = ErrorCode("InvalidId")
        public val Internal: ErrorCode = ErrorCode("Internal")
        public val InvalidPacket: ErrorCode = ErrorCode("InvalidPacket")
        public val Muted: ErrorCode = ErrorCode("Muted")
        public val UnknownUser: ErrorCode = ErrorCode("UnknownUser")
        public val AlreadyFriends: ErrorCode = ErrorCode("AlreadyFriends")
        public val NotFriends: ErrorCode = ErrorCode("NotFriends")
        public val NoInvite: ErrorCode = ErrorCode("NoInvite")
        public val AccountRequired: ErrorCode = ErrorCode("AccountRequired")
        public val UnknownChannel: ErrorCode = ErrorCode("UnknownChannel")
        public val UnknownGroup: ErrorCode = ErrorCode("UnknownGroup")
        public val GroupFull: ErrorCode = ErrorCode("GroupFull")
        public val InvalidName: ErrorCode = ErrorCode("InvalidName")
        public val NotInParty: ErrorCode = ErrorCode("NotInParty")
        public val AlreadyInParty: ErrorCode = ErrorCode("AlreadyInParty")
        public val PartyFull: ErrorCode = ErrorCode("PartyFull")
        public val PartyLocked: ErrorCode = ErrorCode("PartyLocked")
        public val TooLarge: ErrorCode = ErrorCode("TooLarge")
    }
}

@Serializable
@JvmInline
public value class SuccessReason(public val name: String) {
    override fun toString(): String = name

    public companion object {
        public val Login: SuccessReason = SuccessReason("Login")
        public val Ban: SuccessReason = SuccessReason("Ban")
        public val Unban: SuccessReason = SuccessReason("Unban")
        public val Report: SuccessReason = SuccessReason("Report")
        public val Punish: SuccessReason = SuccessReason("Punish")
        public val Pardon: SuccessReason = SuccessReason("Pardon")
        public val Resolve: SuccessReason = SuccessReason("Resolve")
        public val Minecraft: SuccessReason = SuccessReason("Minecraft")
    }
}
